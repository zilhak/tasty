//! 부분 스크롤 영역(DECSTBM) 안에서의 자동 줄바꿈 계약.
//!
//! termwiz `Surface::print_text` 는 scroll region 을 모른다 — 커서가 마지막 화면
//! 행을 넘어설 때만, 그것도 화면 전체를 스크롤한다. 그래서 하단에 입력창을 남기는
//! 부분 영역에서 긴 줄이 자동 줄바꿈되면 커서가 영역 **밖으로** 내려가 입력창을
//! 덮어쓰고, 위로 밀려난 이력은 scrollback 에 적재되지 않은 채 사라졌다
//! (Codex 세션 재개 출력이 긴 URL 중간에서 끊겨 보이던 증상).
//!
//! 이 파일은 실제 ingest 경로(`Terminal::new_detached` + `feed_bytes`)를 몬다.
//! 판정 축은 누적 줄 수가 아니라 **셀 내용 · 순서 · 중복 부재 · wrapped 속성 ·
//! 커서 위치 · 사용자 scroll_offset** 이다.

use tasty_terminal::Terminal;

// ── helpers ──────────────────────────────────────────────────────────────────

fn new_term(cols: usize, rows: usize) -> Terminal {
    let mut t = Terminal::new_detached(cols, rows);
    t.set_scrollback_limit(100_000);
    t
}

fn screen(t: &Terminal) -> Vec<String> {
    let (_cols, rows) = t.dimensions();
    (0..rows).map(|r| t.screen_row(r, true)).collect()
}

fn scrollback_text(t: &Terminal, i: usize) -> String {
    let cells = t.scrollback_line_owned(i).unwrap_or_default();
    let mut s = String::new();
    for (c, _) in &cells {
        s.push_str(c);
    }
    s.trim_end().to_string()
}

fn scrollback(t: &Terminal) -> Vec<String> {
    (0..t.scrollback_len())
        .map(|i| scrollback_text(t, i))
        .collect()
}

/// scrollback(오래된 것부터) ++ 화면 — 마커 계수/순서 판정의 좌변.
fn buffer(t: &Terminal) -> Vec<String> {
    let mut rows = scrollback(t);
    rows.extend(screen(t));
    rows
}

/// 청크 분할 비의존성 판정의 좌변 — (scrollback, 화면, 커서).
type Snapshot = (Vec<String>, Vec<String>, (usize, usize));

/// `chunk` 바이트씩 잘라 넣는다. UTF-8 / 제어 시퀀스 경계를 가로질러도 파서가
/// 같은 결과를 내야 하므로 자르는 위치를 문자 경계에 맞추지 **않는다**.
fn feed_chunked(t: &mut Terminal, bytes: &[u8], chunk: usize) {
    for part in bytes.chunks(chunk.max(1)) {
        t.feed_bytes(part);
    }
}

/// 완료 확인 방법의 결정론 최소 fixture 바이트열.
/// 10열×6행, 5행 `INPUT` / 6행 `STATUS`, `CSI 1;4r` 로 1~4행 영역,
/// 4행 1열에서 `ABCDEFGHIJK` → `CR LF` → `AFTER`.
const MIN_FIXTURE: &[u8] =
    b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1HABCDEFGHIJK\r\nAFTER";

fn min_fixture_term() -> Terminal {
    let mut t = new_term(10, 6);
    t.feed_bytes(MIN_FIXTURE);
    t
}

// ── 최소 fixture ─────────────────────────────────────────────────────────────

/// 영역 하단의 자동 줄바꿈이 영역 안에서 스크롤되는가 — 셀 내용과 커서까지.
#[test]
fn min_fixture_keeps_the_wrap_inside_the_region() {
    let t = min_fixture_term();

    assert_eq!(
        screen(&t),
        vec![
            "".to_string(),
            "ABCDEFGHIJ".to_string(),
            "K".to_string(),
            "AFTER".to_string(),
            "INPUT".to_string(),
            "STATUS".to_string(),
        ],
        "11 번째 문자 K 가 영역 밖(5행)으로 내려가면 안 된다"
    );
    // 커서는 영역(0..=3) 안, `AFTER` 바로 뒤.
    assert_eq!(t.cursor_position(), (5, 3));
}

/// 자동 줄바꿈 **직후**도 관측한다 — CR/LF 가 결과를 덮어 가리지 않도록.
#[test]
fn min_fixture_right_after_the_auto_wrap() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    assert_eq!(t.cursor_position(), (0, 3), "영역 하단에서 시작한다");

    t.feed_bytes(b"ABCDEFGHIJK");
    assert_eq!(
        screen(&t),
        vec![
            "".to_string(),
            "".to_string(),
            "ABCDEFGHIJ".to_string(),
            "K".to_string(),
            "INPUT".to_string(),
            "STATUS".to_string(),
        ],
        "줄바꿈 직후 K 는 영역 하단(3행)에 있어야 한다"
    );
    assert_eq!(t.cursor_position(), (1, 3));
    assert_eq!(t.scrollback_len(), 1, "밀려난 영역 상단 행 하나만 회수된다");
}

/// 밀려난 영역 상단 행만 순서대로 한 번씩 회수되고, 영역 밖 행은 이력에 안 섞인다.
#[test]
fn top_anchored_region_pushes_only_region_rows_in_order() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    // 영역 안(0..=3)에 R0..R3, 영역 밖(4,5)에 INPUT/STATUS.
    t.feed_bytes(b"R0\r\nR1\r\nR2\r\nR3");
    t.feed_bytes(b"\x1b[5;1HINPUT\x1b[6;1HSTATUS");
    t.feed_bytes(b"\x1b[1;4r");
    // 영역 하단(3행) 끝으로 가서 자동 줄바꿈을 두 번 일으킨다.
    t.feed_bytes(b"\x1b[4;3HXXXXXXXXYYYYYYYYYYZ");

    assert_eq!(
        scrollback(&t),
        vec!["R0".to_string(), "R1".to_string()],
        "밀려난 것은 영역 상단 행뿐이고 순서가 보존돼야 한다"
    );
    let text = buffer(&t).join("\n");
    for marker in ["R0", "R1", "R2", "INPUT", "STATUS"] {
        assert_eq!(
            text.matches(marker).count(),
            1,
            "{marker} 는 정확히 한 번만 나와야 한다 (0 = 유실, >1 = 중복)"
        );
    }
    assert_eq!(screen(&t)[4], "INPUT");
    assert_eq!(screen(&t)[5], "STATUS");
}

/// 상단이 0 이 아닌 내부 영역: 밀려난 행은 화면 밖으로 나가는 것이 아니므로
/// 이력에 섞지 않는다 — 명시적 스크롤(`capture_before_scroll`)과 같은 정책이다.
#[test]
fn inner_region_does_not_feed_scrollback() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"HEAD\r\nR1\r\nR2\r\nR3\r\nINPUT\r\nSTATUS");
    t.feed_bytes(b"\x1b[2;4r"); // 영역 (1,3)
    t.feed_bytes(b"\x1b[4;3HXXXXXXXXYY");

    assert_eq!(
        t.scrollback_len(),
        0,
        "화면 밖으로 나가지 않은 행이 이력에 들어가면 안 된다"
    );
    let rows = screen(&t);
    assert_eq!(rows[0], "HEAD", "영역 위 행은 그대로다");
    assert_eq!(rows[4], "INPUT");
    assert_eq!(rows[5], "STATUS");
    assert_eq!(rows[1], "R2");
    assert_eq!(rows[3], "YY", "초과분은 영역 하단에 남는다");
}

// ── 경계 조합 ────────────────────────────────────────────────────────────────

/// 한 줄짜리 영역(top=0): 매 줄바꿈이 그 한 행을 회수한다.
#[test]
fn single_row_region_at_top() {
    let mut t = new_term(4, 4);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[3;1HAA2\x1b[4;1HAA3");
    t.feed_bytes(b"\x1b[1;1r\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");

    let rows = screen(&t);
    assert_eq!(rows[0], "IJ", "마지막 조각만 한 줄 영역에 남는다");
    assert_eq!(rows[1], "");
    assert_eq!(rows[2], "AA2", "영역 밖은 그대로다");
    assert_eq!(rows[3], "AA3");
    assert_eq!(
        scrollback(&t),
        vec!["ABCD".to_string(), "EFGH".to_string()],
        "밀려난 행이 순서대로 전부 회수돼야 한다"
    );
}

/// 한 줄짜리 영역(top≠0): 회수 없이 그 행 안에서만 갱신된다.
#[test]
fn single_row_region_inside_the_screen() {
    let mut t = new_term(4, 4);
    t.feed_bytes(b"\x1b[2J\x1b[HTOP0\x1b[4;1HKEEP");
    t.feed_bytes(b"\x1b[2;2r\x1b[2;1H");
    t.feed_bytes(b"ABCDEFGH");

    let rows = screen(&t);
    assert_eq!(rows[0], "TOP0");
    assert_eq!(rows[1], "EFGH");
    assert_eq!(rows[3], "KEEP");
    assert_eq!(t.scrollback_len(), 0);
}

/// 커서가 영역 **아래**, 화면 마지막 행에 있을 때: 영역도 화면도 스크롤하지
/// 않는다 (VT: 마진 밖 index 는 화면 끝에서 멈춘다). 이력도 안 늘어난다.
#[test]
fn cursor_below_the_region_neither_scrolls_nor_records() {
    let mut t = new_term(4, 4);
    t.feed_bytes(b"\x1b[2J\x1b[HA0\r\nA1\r\nA2");
    t.feed_bytes(b"\x1b[1;3r"); // 영역 (0,2) — 마지막 행 3 은 영역 밖
    t.feed_bytes(b"\x1b[4;1HABCDEFGH");

    let rows = screen(&t);
    assert_eq!(rows[0], "A0", "화면이 스크롤되면 안 된다");
    assert_eq!(rows[1], "A1");
    assert_eq!(rows[2], "A2");
    assert_eq!(rows[3], "EFGH", "줄바꿈이 같은 행에서 되풀이된다");
    assert_eq!(t.scrollback_len(), 0);
}

/// 오른쪽 끝 지연 줄바꿈: 정확히 폭만큼 채워도 그 자체로는 아무것도 안 움직인다.
#[test]
fn deferred_wrap_at_the_right_edge_does_not_scroll_early() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"ABCDEFGHIJ"); // 딱 10열
    assert_eq!(t.scrollback_len(), 0, "폭을 채운 것만으로는 스크롤이 없다");
    assert_eq!(screen(&t)[3], "ABCDEFGHIJ");
    assert_eq!(t.cursor_position(), (10, 3), "커서는 오른쪽 끝에 걸쳐 있다");

    t.feed_bytes(b"K"); // 여기서 비로소 영역 스크롤
    assert_eq!(t.scrollback_len(), 1);
    assert_eq!(screen(&t)[2], "ABCDEFGHIJ");
    assert_eq!(screen(&t)[3], "K");
    assert_eq!(screen(&t)[4], "INPUT");
}

/// CR 은 스크롤을 일으키지 않고, CR 뒤의 출력은 같은 행을 덮어쓴다.
#[test]
fn carriage_return_cancels_the_pending_wrap() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"ABCDEFGHIJ\rZ");
    assert_eq!(t.scrollback_len(), 0, "CR 뒤의 출력은 스크롤을 안 부른다");
    assert_eq!(screen(&t)[3], "ZBCDEFGHIJ");
}

/// 영역 초기화(`CSI r`) 뒤에는 전체 화면 계약으로 돌아간다.
#[test]
fn resetting_the_region_restores_full_screen_scrolling() {
    let mut t = new_term(4, 3);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;2r"); // 영역 (0,1)
    t.feed_bytes(b"\x1b[r"); // 초기화
    t.feed_bytes(b"\x1b[3;1HABCDEFGH");

    // 전체 화면 스크롤이므로 마지막 행에서 자동 줄바꿈할 때 화면이 위로 밀린다.
    assert_eq!(screen(&t)[2], "EFGH");
    assert_eq!(t.scrollback_len(), 1);
}

/// DECOM(origin mode)과 함께 써도 자동 줄바꿈이 영역 안에 머문다.
#[test]
fn origin_mode_keeps_the_wrap_inside_the_region() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS");
    t.feed_bytes(b"\x1b[1;4r\x1b[?6h"); // 영역 (0,3) + origin mode
    t.feed_bytes(b"\x1b[4;1H"); // origin mode 기준 4행 = 절대 4행 (top=0)
    t.feed_bytes(b"ABCDEFGHIJK");

    let rows = screen(&t);
    assert_eq!(rows[2], "ABCDEFGHIJ");
    assert_eq!(rows[3], "K");
    assert_eq!(rows[4], "INPUT");
    assert_eq!(rows[5], "STATUS");
}

/// 동기 출력(DEC 2026) 로 감싸도 결과가 같다.
#[test]
fn synchronized_output_does_not_change_the_result() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"\x1b[?2026h");
    t.feed_bytes(b"ABCDEFGHIJK\r\nAFTER");
    t.feed_bytes(b"\x1b[?2026l");

    assert_eq!(screen(&t), screen(&min_fixture_term()));
}

/// resize 는 영역을 초기화한다 — 낡은 영역으로 판정하지 않는다.
#[test]
fn resize_clears_the_region_and_keeps_wrapping_sane() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;4r\x1b[4;1H");
    t.resize(10, 4);
    t.feed_bytes(b"\x1b[4;1HABCDEFGHIJK");

    // 영역이 없으므로 전체 화면 스크롤 — 영역 판정이 낡았다면 여기서 어긋난다.
    let rows = screen(&t);
    assert_eq!(rows[2], "ABCDEFGHIJ");
    assert_eq!(rows[3], "K");
}

// ── 대체 화면 ────────────────────────────────────────────────────────────────

/// 대체 화면에서도 같은 영역 계약이 성립하고, 그 출력이 primary 이력을
/// 오염시키지 않는다.
#[test]
fn alternate_screen_honors_the_region_without_touching_primary_history() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[HPRIMARY");
    t.feed_bytes(b"\x1b[?1049h"); // 대체 화면
    t.feed_bytes(b"\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"ABCDEFGHIJK\r\nAFTER");

    assert!(t.is_alternate_screen());
    assert_eq!(
        screen(&t),
        vec![
            "".to_string(),
            "ABCDEFGHIJ".to_string(),
            "K".to_string(),
            "AFTER".to_string(),
            "INPUT".to_string(),
            "STATUS".to_string(),
        ],
        "대체 화면에서도 영역 밖 입력창이 보존돼야 한다"
    );
    assert_eq!(
        t.scrollback_len(),
        0,
        "대체 화면 출력은 primary 이력에 들어가면 안 된다"
    );

    t.feed_bytes(b"\x1b[?1049l");
    assert_eq!(screen(&t)[0], "PRIMARY", "primary 화면이 그대로 돌아온다");
    assert_eq!(t.scrollback_len(), 0);
}

// ── 청크 분할 비의존성 ───────────────────────────────────────────────────────

/// 같은 바이트를 1 / 4096 / 65536 바이트로 잘라 넣어도 결과가 같다.
/// 자르는 위치를 문자 경계에 맞추지 않으므로 UTF-8·제어 시퀀스도 가로지른다.
#[test]
fn chunking_does_not_change_the_result() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1b[2J\x1b[H\x1b[39;1HINPUT\x1b[40;1HSTATUS\x1b[1;38r\x1b[38;1H");
    bytes.extend_from_slice(
        "https://developer.mozilla.org/en-US/docs/Web/HTML/Element/Heading_Elements#in-page_navigation"
            .as_bytes(),
    );
    bytes.extend_from_slice(
        "\r\n한글전각テスト\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}e\u{0301}MARK1".as_bytes(),
    );
    bytes.extend_from_slice(b"\r\nMARK2\r\nMARK3");

    let mut reference: Option<Snapshot> = None;
    for chunk in [1usize, 4096, 65536] {
        let mut t = new_term(84, 40);
        feed_chunked(&mut t, &bytes, chunk);
        let got = (scrollback(&t), screen(&t), t.cursor_position());
        match &reference {
            None => reference = Some(got),
            Some(want) => assert_eq!(&got, want, "chunk={chunk} 에서 결과가 갈렸다"),
        }
    }
}

// ── 넓은 글자 / 결합 문자 ────────────────────────────────────────────────────

/// 전각·결합 문자·emoji 가 영역 하단에서 줄바꿈해도 영역 밖으로 안 나간다.
#[test]
fn wide_and_combining_graphemes_wrap_inside_the_region() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    // 전각 5 자로 10 열을 정확히 채운 뒤 결합 문자 + emoji 로 줄바꿈시킨다.
    t.feed_bytes("한글전각テ".as_bytes());
    assert_eq!(t.scrollback_len(), 0);
    t.feed_bytes("e\u{0301}\u{1f44d}".as_bytes());

    let rows = screen(&t);
    assert_eq!(rows[2], "한글전각テ");
    assert_eq!(rows[4], "INPUT", "영역 밖 입력창이 살아 있어야 한다");
    assert_eq!(rows[5], "STATUS");
    assert!(
        rows[3].contains('\u{1f44d}'),
        "줄바꿈된 조각은 영역 하단(3행)에 있어야 한다: {:?}",
        rows[3]
    );
}

// ── wrapped 속성 / 사용자 scroll_offset ──────────────────────────────────────

/// 자동 줄바꿈으로 밀려난 행은 `wrapped`(논리적 연속) 로, 명시적 개행으로 밀려난
/// 행은 그렇지 않게 기록된다.
#[test]
fn wrapped_flag_distinguishes_soft_wrap_from_hard_newline() {
    let mut t = new_term(4, 3);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;2r\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHI"); // 영역 하단까지 내려가 한 번 회수
    assert_eq!(scrollback(&t), vec!["ABCD".to_string()]);
    assert_eq!(
        t.scrollback_line_wrapped(0),
        Some(true),
        "오른쪽 끝을 채우고 넘어간 행은 논리적 연속이다"
    );

    let mut t2 = new_term(4, 3);
    t2.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;2r\x1b[1;1H");
    t2.feed_bytes(b"AB\r\nCD\r\nEF\r\nGH");
    assert_eq!(scrollback(&t2), vec!["AB".to_string(), "CD".to_string()]);
    assert_eq!(t2.scrollback_line_wrapped(0), Some(false));
}

/// 사용자가 위로 스크롤해 둔 상태에서 영역 자동 줄바꿈이 일어나도 보고 있던
/// 위치가 밀리지 않는다 — 늘어난 이력 수만큼 offset 이 보정된다.
#[test]
fn user_scroll_offset_is_compensated() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"SEED______X"); // 이력 1 줄 만들어 스크롤 가능하게
    assert_eq!(t.scrollback_len(), 1);
    t.scroll_up(1);
    assert_eq!(t.scroll_offset(), 1);

    t.feed_bytes(b"\rABCDEFGHIJKLMNOPQRSTU"); // 영역 스크롤 2 회
    assert_eq!(t.scrollback_len(), 3);
    assert_eq!(
        t.scroll_offset(),
        3,
        "이력이 2 줄 늘었으면 offset 도 2 만큼 따라와야 한다"
    );
}

// ── DECAWM ───────────────────────────────────────────────────────────────────

/// DECAWM(`CSI ?7l`) 은 **현재 미지원** 이다 — termwiz Surface 가 언제나 자동
/// 줄바꿈한다. 이 테스트는 그 현황을 못박아 이번 수정의 회귀와 구분한다:
/// 줄바꿈은 여전히 일어나지만, 그래도 영역 밖으로 나가지는 않는다.
#[test]
fn autowrap_off_is_unsupported_but_still_region_confined() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[5;1HINPUT\x1b[6;1HSTATUS\x1b[1;4r\x1b[4;1H");
    t.feed_bytes(b"\x1b[?7l"); // DECAWM off — 무시된다
    t.feed_bytes(b"ABCDEFGHIJK");

    let rows = screen(&t);
    assert_eq!(rows[3], "K", "미지원: 줄바꿈이 여전히 일어난다");
    assert_eq!(rows[4], "INPUT", "그래도 영역 밖은 안 건드린다");
    assert_eq!(rows[5], "STATUS");
}

// ── 통합 fixture (Codex 재개 패턴) ───────────────────────────────────────────

/// 84열×40행, 하단 두 행을 입력창으로 남기는 부분 영역에 84열보다 긴 링크와
/// 후속 메시지 표식을 흘린다. 실제 재개 출력이 긴 URL 중간에서 끊겨 보이던
/// 그 조합이다 — 링크 이후 모든 표식이 원래 순서로 남아야 한다.
#[test]
fn long_link_then_following_messages_all_survive() {
    const LINK: &str = "https://developer.mozilla.org/en-US/docs/Web/HTML/Element/Heading_Elements#in-page_navigation_and_table_of_contents";
    let markers = [
        "MSG-todo-request",
        "MSG-todo-done",
        "MSG-recheck",
        "MSG-final",
    ];

    let mut t = new_term(84, 40);
    t.feed_bytes(b"\x1b[2J\x1b[H");
    t.feed_bytes(b"\x1b[39;1H> input bar\x1b[40;1H-- status --");
    t.feed_bytes(b"\x1b[1;38r"); // 영역 (0,37) — 39/40 행은 입력창
    t.feed_bytes(b"\x1b[38;1H");
    // 영역을 한 번 다 채워 상단 회수가 실제로 일어나게 한다.
    for i in 0..40 {
        t.feed_bytes(format!("filler {i:02}\r\n").as_bytes());
    }
    t.feed_bytes(LINK.as_bytes());
    t.feed_bytes(b"\r\n");
    for m in markers {
        t.feed_bytes(format!("{m}\r\n").as_bytes());
    }

    let rows = buffer(&t);
    let joined = rows.join("\n");
    // 링크는 84 열에서 접히므로 두 행으로 나뉘어 남는다 — 두 조각이 다 있어야
    // 링크가 통째로 보존된 것이다.
    let (link_head, link_tail) = LINK.split_at(84);
    assert!(
        rows.iter().any(|r| r == link_head),
        "링크 앞 84 열이 한 행으로 남아야 한다"
    );
    assert!(
        rows.iter().any(|r| r == link_tail),
        "줄바꿈된 링크 뒷조각이 남아야 한다"
    );
    let mut last = None;
    for m in markers {
        assert_eq!(
            joined.matches(m).count(),
            1,
            "{m} 는 정확히 한 번 (0 = 유실, >1 = 중복)"
        );
        let at = rows
            .iter()
            .position(|r| r.contains(m))
            .expect("위에서 계수했다");
        if let Some(prev) = last {
            assert!(at > prev, "{m} 가 원래 순서를 벗어났다");
        }
        last = Some(at);
    }

    let vis = screen(&t);
    assert_eq!(vis[38], "> input bar", "입력창이 유지돼야 한다");
    assert_eq!(vis[39], "-- status --");
    let (_cx, cy) = t.cursor_position();
    assert!(cy <= 37, "커서가 영역 안에 남아야 한다 (row={cy})");

    // 입력창이 이력으로 복제되지도 않았다.
    for m in ["> input bar", "-- status --"] {
        assert_eq!(scrollback(&t).join("\n").matches(m).count(), 0);
    }
}

/// 저장 → 재시작 복원 경로(`scrollback_lines_all` ++ `screen_snapshot_lines`
/// 을 새 터미널에 `inject_scrollback`)를 지나도 보존된 출력이 유지된다.
#[test]
fn saved_and_restored_output_keeps_the_recovered_history() {
    let markers = ["MSG-a", "MSG-b", "MSG-c"];
    let mut t = new_term(20, 8);
    t.feed_bytes(b"\x1b[2J\x1b[H\x1b[8;1H> input\x1b[1;7r\x1b[7;1H");
    for i in 0..10 {
        t.feed_bytes(format!("line {i:02} tail-that-wraps-past-the-right-edge\r\n").as_bytes());
    }
    for m in markers {
        t.feed_bytes(format!("{m}\r\n").as_bytes());
    }

    let mut saved = t.scrollback_lines_all();
    saved.extend(t.screen_snapshot_lines());

    let mut restored = new_term(20, 8);
    restored.inject_scrollback(saved);

    let text: String = (0..restored.scrollback_len())
        .map(|i| scrollback_text(&restored, i))
        .collect::<Vec<_>>()
        .join("\n");
    for m in markers {
        assert_eq!(
            text.matches(m).count(),
            1,
            "{m} 가 저장·복원을 지나 정확히 한 번 남아야 한다"
        );
    }
}

/// DECSTBM 이 **없는** 대체 화면 — full-screen TUI(vim/less/htop)의 정상 경로다.
/// 위 테스트는 영역을 깔아 영역 스크롤 갈래만 밟으므로 이 갈래를 한 번도 안 잰다.
/// 여기서 재는 것: 대체 화면의 자동 줄바꿈이 화면 전체를 미는 갈래에서도 primary
/// 이력·reflow 데이터·사용자 뷰포트가 그대로여야 한다.
#[test]
fn alternate_full_screen_wrap_does_not_touch_primary_history() {
    let mut t = new_term(20, 5);
    t.feed_bytes(b"\x1b[2J\x1b[HPRIMARY-HISTORY");
    // primary 에 이력 한 줄을 만들고 사용자가 위로 스크롤해 둔 상태로 진입한다 —
    // 대체 화면이 이력을 밀면 이 offset 도 함께 밀린다.
    t.feed_bytes(b"\x1b[5;1HSEED\r\nSEED2");
    let primary_screen = screen(&t);
    let primary_scrollback = scrollback(&t);
    assert_eq!(primary_scrollback.len(), 1, "이력 한 줄을 만들어 두었다");
    t.scroll_up(1);

    t.feed_bytes(b"\x1b[?1049h"); // 대체 화면 — DECSTBM 없음
    t.feed_bytes(b"\x1b[5;1H");
    // 마지막 행에서 두 번 접히게 흘린다. 영역이 없으므로 termwiz 가 화면 전체를
    // 미는 갈래로 간다.
    t.feed_bytes(b"one_________________two_________________three");

    assert_eq!(
        scrollback(&t),
        primary_scrollback,
        "대체 화면 내용이 primary 이력에 새면 안 된다"
    );
    assert_eq!(
        t.scroll_offset(),
        1,
        "대체 화면 출력이 primary 뷰포트를 밀면 안 된다"
    );

    t.feed_bytes(b"\x1b[?1049l");
    assert_eq!(
        scrollback(&t),
        primary_scrollback,
        "대체 화면을 나온 뒤에도 이력은 들어가기 전 그대로여야 한다"
    );
    assert_eq!(screen(&t), primary_screen, "primary 화면도 그대로다");
}

/// 화면 밖 하단 마진(`CSI 3;100r` — 6행 화면)은 저장 시점에 마지막 행으로
/// 클램프된다(xterm `CASE_DECSTBM`: `bot > MaxRows(screen)` 이면 `MaxRows`).
/// 그래서 명시 개행과 자동 줄바꿈이 **같은 영역**을 민다 — 클램프가 읽는 쪽에만
/// 있으면 두 경로가 서로 다른 행을 스크롤한다.
#[test]
fn an_out_of_range_bottom_margin_scrolls_the_same_rows_for_lf_and_wrap() {
    let seed = b"\x1b[2J\x1b[HA0\r\nA1\r\nA2\r\nA3\r\nA4\r\nA5\x1b[3;100r";

    let mut lf = new_term(10, 6);
    lf.feed_bytes(seed);
    lf.feed_bytes(b"\x1b[6;1HLF________\r\n");

    let mut wrap = new_term(10, 6);
    wrap.feed_bytes(seed);
    wrap.feed_bytes(b"\x1b[6;1HLF________X");

    // 영역은 3~6행(0-based 2..=5). 두 경로 모두 그 영역만 밀어 A2 를 떨구고
    // 영역 밖(0,1행)의 A0/A1 은 남긴다.
    assert_eq!(&lf.screen_row(0, true), "A0");
    assert_eq!(&lf.screen_row(1, true), "A1");
    assert_eq!(&lf.screen_row(2, true), "A3", "A2 가 영역에서 밀려났다");
    assert_eq!(&lf.screen_row(4, true), "LF________");
    assert_eq!(
        screen(&wrap)[..5],
        screen(&lf)[..5],
        "자동 줄바꿈이 명시 개행과 같은 영역을 밀어야 한다"
    );
    // 내부 영역(top≠0)이라 어느 쪽도 이력에 넣지 않는다.
    assert_eq!(lf.scrollback_len(), 0);
    assert_eq!(wrap.scrollback_len(), 0);
}

/// 역전 마진(`CSI 6;3r`)도 같은 경계로 정규화된다 — 저장 시점에 `top <= bottom`
/// 이 보장되므로 `bottom - top` 을 쓰는 자리가 underflow 하지 않는다.
#[test]
fn inverted_margins_normalize_to_one_row_for_both_paths() {
    let seed = b"\x1b[2J\x1b[HB0\r\nB1\r\nB2\r\nB3\r\nB4\r\nB5\x1b[6;3r";

    let mut lf = new_term(10, 6);
    lf.feed_bytes(seed);
    lf.feed_bytes(b"\x1b[6;1HZZ\r\n");

    let mut wrap = new_term(10, 6);
    wrap.feed_bytes(seed);
    wrap.feed_bytes(b"\x1b[6;1HZZ________Y");

    // 한 줄 영역(6행)이라 그 행만 갱신되고 위 행들은 그대로다.
    for (r, want) in [(0, "B0"), (1, "B1"), (2, "B2"), (3, "B3"), (4, "B4")] {
        assert_eq!(&lf.screen_row(r, true), want, "row {r}");
        assert_eq!(&wrap.screen_row(r, true), want, "row {r}");
    }
    assert_eq!(lf.scrollback_len(), 0);
    assert_eq!(wrap.scrollback_len(), 0);

    // 세 번째 읽는 쪽 — DECOM 의 절대 행 해석도 같은 경계를 써야 한다. 정규화가
    // 없으면 저장된 영역이 `(5, 2)` 로 뒤집힌 채 남아 `resolve_origin_row` 만
    // 하단 2 로 클램프하고, 같은 영역인데 커서만 다른 행에 앉는다.
    let mut om = new_term(10, 6);
    om.feed_bytes(seed);
    om.feed_bytes(b"\x1b[?6h\x1b[1;1H");
    assert_eq!(
        om.cursor_position(),
        (0, 5),
        "origin mode 의 1 행은 정규화된 영역 상단이어야 한다"
    );
}

/// 상단 마진까지 화면 밖인 요청(`CSI 100;200r` — 6행 화면)도 그리드 안으로
/// 정규화된다. 상단을 안 클램프하면 하단 정규화의 `clamp(top, last)` 가 하한이
/// 상한보다 커져 그 자리에서 패닉한다 — 원격 프로그램이 보낸 한 줄이 파서
/// 스레드를 죽이는 형태라, 조용한 오동작이 아니라 크래시다.
#[test]
fn a_top_margin_past_the_last_row_is_normalized_not_a_panic() {
    let mut t = new_term(10, 6);
    t.feed_bytes(b"\x1b[2J\x1b[HC0\r\nC1\r\nC2\r\nC3\r\nC4\r\nC5");
    t.feed_bytes(b"\x1b[100;200r");
    // 마지막 행 하나짜리 영역으로 접힌다. 자동 줄바꿈도 명시 개행도 그 행만 민다.
    t.feed_bytes(b"\x1b[6;1HDD________E");
    for (r, want) in [(0, "C0"), (1, "C1"), (2, "C2"), (3, "C3"), (4, "C4")] {
        assert_eq!(&t.screen_row(r, true), want, "row {r}");
    }
    assert_eq!(&t.screen_row(5, true), "E");
    assert_eq!(
        t.scrollback_len(),
        0,
        "영역 상단이 0 이 아니라 회수 대상이 아니다"
    );
}

/// 부분 영역 안에서 EL1(`CSI 1K`)이 걸친 커서를 만나도 **영역을 스크롤하지 않는다.**
///
/// 소거는 커서를 움직이지 않는 연산이고 지우는 범위는 커서 칸까지다. 걸친 커서
/// (`cursor_position()` 의 열이 `cols`)는 마지막 열에 올라앉은 것이므로 행 전체
/// (`cols` 칸)가 범위이지 그보다 한 칸 많지 않다 — 한 칸이 더 찍히면 그것이 다음
/// 행으로 넘어가 **소거 명령이 줄바꿈을 일으킨다.** 부분 영역에서는 그 줄바꿈이
/// 영역 스크롤 한 번 + 빈 이력 한 줄로 드러나므로, 이 파일이 그 자리를 잰다.
/// 경계 네 자리와 전체 화면 케이스는 `erase_boundaries.rs` 가 따로 잰다.
#[test]
fn el1_at_a_parked_cursor_does_not_scroll_the_region() {
    let mut parked = new_term(10, 6);
    parked.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;4r\x1b[4;1H");
    parked.feed_bytes(b"ABCDEFGHIJ"); // 딱 10열 — 커서가 오른쪽 끝에 걸친다
    assert_eq!(parked.cursor_position(), (10, 3));
    parked.feed_bytes(b"\x1b[1K");
    assert_eq!(
        parked.scrollback_len(),
        0,
        "EL1 은 소거 연산이다 — 영역을 스크롤하지도 이력을 쌓지도 않는다"
    );
    assert_eq!(&parked.screen_row(3, true), "", "커서 행이 비워진다");
    assert_eq!(
        parked.cursor_position(),
        (10, 3),
        "걸친 상태가 보존된다 — 소거가 커서를 움직이지 않는다"
    );

    // 걸치지 않았으면(9자) 그 전부터 아무 일도 없었다 — 원인이 영역이 아니라 소거
    // 범위였음을 가르는 대조군이다.
    let mut inside = new_term(10, 6);
    inside.feed_bytes(b"\x1b[2J\x1b[H\x1b[1;4r\x1b[4;1H");
    inside.feed_bytes(b"ABCDEFGHI");
    inside.feed_bytes(b"\x1b[1K");
    assert_eq!(inside.scrollback_len(), 0);
}
