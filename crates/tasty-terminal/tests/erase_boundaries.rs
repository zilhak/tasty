//! 소거 명령(ED/EL)이 커서 칸을 어떻게 세는가 — 경계 네 자리.
//!
//! ED/EL 은 커서를 움직이지 않고, 지우는 범위는 **커서가 올라앉은 칸을 포함**한다.
//! 그 두 문장이 경계에서 갈리는 자리는 둘이다.
//!
//! - **0 열**: 포함이므로 지울 칸은 0 이 아니라 **하나**다.
//! - **걸친 커서**: 자동 줄바꿈이 대기 중인 상태를 이 구현은 `cursor_position()` 의
//!   열이 화면 폭과 같은 값(`cx == cols`)인 것으로 나타낸다. 그 칸은 그리드에 없고
//!   커서가 실제로 올라앉은 칸은 마지막 열이므로, EL1 의 범위는 행 전체(`cols` 칸)
//!   이지 그보다 한 칸 많지 않다. 한 칸이 더 찍히면 그것이 다음 행으로 넘어가
//!   **소거 명령이 줄바꿈을 일으킨다.**
//!
//! 걸친 상태 자체도 관측 대상이다. `Position::Absolute(cols)` 는 `cols - 1` 로 잘려
//! 어떤 커서 이동 `Change` 로도 그 자리에 갈 수 없으므로, 소거가 그 상태를 풀어버리면
//! 다음 글자가 줄바꿈 없이 마지막 칸을 덮어쓴다. 그래서 이 파일은 지워진 칸 수와
//! **소거 직후 한 글자를 더 먹였을 때 그것이 어디에 놓이는가**를 함께 잰다.
//!
//! 영역(DECSTBM) 맥락의 같은 경계는 `region_autowrap.rs` 가 잰다 — 이 파일은 영역
//! 없는 전체 화면이다.

use tasty_terminal::Terminal;

const COLS: usize = 10;

fn term() -> Terminal {
    let mut t = Terminal::new_detached(COLS, 4);
    t.set_scrollback_limit(1000);
    t
}

/// 첫 행을 `fill` 로 채우고 커서를 `col`(0-based) 로 옮긴 터미널.
///
/// `col == COLS` 는 커서 이동으로 갈 수 없는 자리라 **정확히 `COLS` 칸을 찍어**
/// 만든다(그것이 그 자리에 닿는 유일한 길이다).
fn at_column(fill: &str, col: usize) -> Terminal {
    let mut t = term();
    if col == COLS {
        t.feed_bytes(fill.as_bytes());
        assert_eq!(
            t.cursor_position(),
            (COLS, 0),
            "준비 실패: {COLS} 칸을 찍으면 커서가 걸쳐야 한다"
        );
    } else {
        t.feed_bytes(fill.as_bytes());
        t.feed_bytes(format!("\x1b[1;{}H", col + 1).as_bytes());
        assert_eq!(t.cursor_position(), (col, 0), "준비 실패: 커서 열");
    }
    t
}

// ── EL1 (`CSI 1K`) — 행 시작 ~ 커서 칸 ────────────────────────────────────────

#[test]
fn el1_at_column_zero_erases_the_cursor_cell_itself() {
    let mut t = at_column("ABCDEFGHI", 0);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(
        &t.screen_row(0, true),
        " BCDEFGHI",
        "0 열 커서도 커서 칸이 범위에 든다 — 한 칸이 지워진다"
    );
    assert_eq!(t.cursor_position(), (0, 0), "소거는 커서를 움직이지 않는다");
}

#[test]
fn el1_at_column_one_erases_two_cells() {
    let mut t = at_column("ABCDEFGHI", 1);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(&t.screen_row(0, true), "  CDEFGHI");
    assert_eq!(t.cursor_position(), (1, 0));
}

#[test]
fn el1_at_the_last_column_erases_the_whole_line() {
    let mut t = at_column("ABCDEFGHI", COLS - 1);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(t.cursor_position(), (COLS - 1, 0));
}

#[test]
fn el1_at_a_parked_cursor_erases_the_line_without_wrapping() {
    // 다음 행에 표식을 깔아 둔다. 한 칸이 넘어가면 그 표식의 첫 글자가 공백이 되므로,
    // 빈 행을 확인하는 것과 달리 **넘어간 칸을 직접 본다**(빈 행은 공백 한 칸을
    // 받아도 `screen_row` 의 우측 트림 때문에 여전히 빈 문자열이다).
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[1K");

    assert_eq!(&t.screen_row(0, true), "", "커서 행 전체가 지워진다");
    assert_eq!(
        &t.screen_row(1, true),
        "QRSTUVWXYZ",
        "다음 행은 건드려지지 않는다 — 한 칸이 넘어가지 않았다"
    );
    assert_eq!(t.scrollback_len(), 0, "소거가 화면을 스크롤하지 않는다");
}

#[test]
fn el1_keeps_the_pending_wrap_so_the_next_glyph_still_wraps() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "걸친 상태가 보존된다 — 소거는 커서를 움직이지 않는다"
    );

    t.feed_bytes(b"Z");
    assert_eq!(
        (t.screen_row(0, true), t.screen_row(1, true)),
        (String::new(), "Z".to_string()),
        "대기 중이던 줄바꿈이 다음 글자에서 일어난다"
    );
    assert_eq!(t.cursor_position(), (1, 1));
}

// ── 전각·결합문자 ────────────────────────────────────────────────────────────

#[test]
fn el1_at_a_parked_cursor_after_wide_glyphs_behaves_the_same() {
    // **이 시험이 홀로 지키는 것은 소거가 아니라 폭 계산 쪽 가정이다.** 소거 코드가
    // 보는 값은 `(cx, cols)` 뿐이고 여기서는 `(10, 10)` — ASCII 로 걸치게 만든 위
    // 시험과 **완전히 같은 입력**이다. 그래서 소거 코드의 어떤 변이도 이 시험만 따로
    // 죽이지 못한다(변이 셋으로 확인: 상한 클램프 제거 · 걸친 커서에도 복원 · 범위
    // 한 칸 축소 — 죽을 때는 늘 다른 시험과 함께 죽었다). 이 시험이 실제로 못박는
    // 것은 **전각 5 글자가 10 칸을 채우고 그때 커서가 걸친다**는 ingest 쪽 가정이다.
    // (다만 위 시험과 무조건 함께 죽지는 않는다 — 위 시험은 소거 뒤 커서를 안 보고
    // 이 시험은 보므로, 걸친 상태만 푸는 변이에서는 이쪽만 죽는다.)
    //
    // 2 칸짜리 5 글자가 정확히 10 열을 채운다.
    let mut t = at_column("가나다라마", COLS);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(&t.screen_row(1, true), "");
    assert_eq!(t.cursor_position(), (COLS, 0));

    t.feed_bytes("힣".as_bytes());
    assert_eq!(
        &t.screen_row(1, true),
        "힣",
        "걸친 상태가 전각에서도 보존된다"
    );
}

#[test]
fn el1_stopping_inside_a_wide_glyph_leaves_no_half_cell() {
    // "가나다" = 열 0..5. 커서를 4 열(= "다" 의 첫 칸)에 두면 소거 범위가 그 글자를
    // 반으로 가른다. 잘린 나머지 칸이 남으면 화면에 깨진 글리프가 보인다.
    let mut t = at_column("가나다", 4);
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(
        &t.screen_row(0, true),
        "",
        "쪼개진 전각의 남은 칸도 함께 비워진다"
    );
    assert_eq!(t.cursor_position(), (4, 0));
}

#[test]
fn el1_counts_cells_not_graphemes_around_a_combining_mark() {
    // `e` + U+0301 은 이 ingest 경로에서 **두 칸**을 차지한다(셀 폭과 grapheme 폭이
    // 갈리는 알려진 자리 — `scrollback_capture.rs` 의 폭 비교 테스트가 그 갈림을
    // 따로 센다). 소거는 grapheme 이 아니라 **칸**을 세므로, 여기서 재는 것은
    // "커서 열까지 몇 칸이 지워지는가" 이지 결합문자를 어떻게 묶는가가 아니다.
    let mut t = term();
    t.feed_bytes("e\u{0301}abc".as_bytes());
    let after_text = t.cursor_position();
    assert_eq!(after_text.0, 5, "준비 실패: 이 경로의 셀 배치가 바뀌었다");

    t.feed_bytes(b"\x1b[1;3H\x1b[1K"); // 커서를 2 열로 → 0..=2 세 칸
    assert_eq!(&t.screen_row(0, true), "   bc");
    assert_eq!(t.cursor_position(), (2, 0));
}

// ── ED1 (`CSI 1J`) — 화면 시작 ~ 커서 칸 ──────────────────────────────────────
//
// EL1 과 **같은 범위 계산**을 쓰는 형제다. 커서 위의 행들은 통째로 지우고 커서 행은
// 0 열부터 커서 칸까지 지운다. 그래서 경계도 같은 두 자리에서 갈린다.

#[test]
fn ed1_at_column_zero_erases_the_cursor_cell_itself() {
    let mut t = term();
    t.feed_bytes(b"ABCDEFGHI\x1b[2;1HJKL\x1b[1;1H");
    t.feed_bytes(b"\x1b[1J");
    assert_eq!(&t.screen_row(0, true), " BCDEFGHI");
    assert_eq!(&t.screen_row(1, true), "JKL", "커서 행 아래는 남는다");
    assert_eq!(t.cursor_position(), (0, 0));
}

#[test]
fn ed1_erases_every_row_above_the_cursor_and_up_to_it() {
    let mut t = term();
    t.feed_bytes(b"\x1b[1;1HAAAA\x1b[2;1HBBBB\x1b[3;1HCCCC\x1b[2;3H");
    t.feed_bytes(b"\x1b[1J");
    assert_eq!(&t.screen_row(0, true), "", "위 행은 통째로 지워진다");
    assert_eq!(&t.screen_row(1, true), "   B", "커서 행은 커서 칸까지");
    assert_eq!(&t.screen_row(2, true), "CCCC", "아래 행은 그대로");
    assert_eq!(t.cursor_position(), (2, 1));
}

#[test]
fn ed1_at_a_parked_cursor_erases_the_cursor_row_too() {
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[1J");
    assert_eq!(
        &t.screen_row(0, true),
        "",
        "걸친 커서는 마지막 열에 올라앉은 것이므로 커서 행 전체가 범위다"
    );
    assert_eq!(
        &t.screen_row(1, true),
        "QRSTUVWXYZ",
        "한 칸이 다음 행으로 넘어가지 않는다"
    );
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "걸친 상태가 보존된다 — 소거가 커서를 움직이지 않는다"
    );
}

// ── EL2 (`CSI 2K`) · ED2 (`CSI 2J`) — 범위는 커서와 무관, 커서는 제자리 ──────────
//
// 이 둘은 지울 범위가 커서 위치에 안 달렸으므로 "커서 칸을 포함하는가" 는 물을 것이
// 없다. 대신 **소거가 커서를 움직이지 않는가** 하나만 남는다. 둘 다 termwiz 의 지우기
// primitive 로 지우는데 그 primitive 가 커서를 0 열/홈으로 끌고 가므로, 구현이 원래
// 자리로 되돌려야 계약이 지켜진다.

#[test]
fn el2_does_not_move_the_cursor() {
    let mut t = at_column("ABCDEFGHI", 4);
    t.feed_bytes(b"\x1b[2K");
    assert_eq!(&t.screen_row(0, true), "", "EL2 는 행 전체를 지운다");
    assert_eq!(
        t.cursor_position(),
        (4, 0),
        "EL2 는 커서를 0 열로 옮기지 않는다"
    );
    t.feed_bytes(b"Z");
    assert_eq!(
        &t.screen_row(0, true),
        "    Z",
        "다음 글자는 커서가 있던 열에 놓인다"
    );
}

#[test]
fn el2_at_a_parked_cursor_lands_on_the_last_column() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[2K");
    assert_eq!(&t.screen_row(0, true), "");
    // 걸친 자리(`cx == COLS`)는 `Position::Absolute` 로 못 가리키고, 다시 걸치게 할
    // 글자를 찍으면 그 칸만 pen 이 달라진다. 그래서 EL2 는 마지막 열까지만 되돌리고
    // 대기 중이던 줄바꿈은 풀린다 — 0 열로 끌려가던 것보다 가깝지만 완전하지는 않다.
    assert_eq!(t.cursor_position(), (COLS - 1, 0));
    t.feed_bytes(b"Z");
    assert_eq!(&t.screen_row(0, true), "         Z");
    assert_eq!(
        &t.screen_row(1, true),
        "",
        "대기 중이던 줄바꿈은 풀렸으므로 다음 행으로 넘어가지 않는다"
    );
}

#[test]
fn ed2_does_not_move_the_cursor() {
    let mut t = term();
    t.feed_bytes(b"line1\r\nline2\r\nline3");
    t.feed_bytes(b"\x1b[2;5H");
    assert_eq!(t.cursor_position(), (4, 1), "준비 실패: 커서 자리");
    t.feed_bytes(b"\x1b[2J");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(&t.screen_row(1, true), "");
    assert_eq!(&t.screen_row(2, true), "");
    assert_eq!(
        t.cursor_position(),
        (4, 1),
        "ED2 는 커서를 홈으로 옮기지 않는다"
    );
    t.feed_bytes(b"Z");
    assert_eq!(&t.screen_row(1, true), "    Z");
}

#[test]
fn ed2_at_a_parked_cursor_lands_on_the_last_column() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[2J");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(t.cursor_position(), (COLS - 1, 0));
    t.feed_bytes(b"Z");
    assert_eq!(&t.screen_row(0, true), "         Z");
    assert_eq!(&t.screen_row(1, true), "");
}

// ── 대체 화면 ────────────────────────────────────────────────────────────────

/// 대체 화면에서는 `scrollback_len()` 이 "줄바꿈이 안 났다" 를 증명하지 못한다 —
/// `scrollback.rs` 의 두 스크롤 갈래가 대체 화면일 때 **capture 만** 건너뛰고 스크롤
/// 자체는 그대로 일으키기 때문이다. 그래서 이 시험은 스크롤백 길이가 아니라 **화면 행
/// 내용과 커서 자리**로 잰다(EL1 을 쓰는 TUI 는 대부분 대체 화면에 있다).
#[test]
fn el1_at_a_parked_cursor_does_not_shift_the_alternate_screen() {
    let mut t = term();
    t.feed_bytes(b"\x1b[?1049h");
    t.feed_bytes(b"ABCDEFGHIJ");
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ");
    t.feed_bytes(b"\x1b[1;1HABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 대체 화면에서도 {COLS} 칸을 찍으면 커서가 걸친다"
    );
    t.feed_bytes(b"\x1b[1K");
    assert_eq!(&t.screen_row(0, true), "", "커서 행은 전부 지워진다");
    assert_eq!(
        &t.screen_row(1, true),
        "QRSTUVWXYZ",
        "화면이 한 줄도 밀리지 않았다"
    );
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태가 보존된다");
}

// ── EL0 (`CSI 0K`) · ED0 (`CSI 0J`) — 걸친 커서에서 지우다 만다 (기록) ─────────
//
// 아래 둘은 **현재 동작을 값으로 못박는 기록 시험**이다. 위 표(문서의 "소거 명령과 걸친
// 커서")가 이 둘을 "미해결" 이라는 이름으로 세는데, 그 이름만으로는 아무도 안 운다 —
// 누가 고쳐도, 더 나쁘게 만들어도 초록이다. 그래서 `region_autowrap.rs` 가 쓴 관례를
// 그대로 쓴다: **이름에 현재 상태를 적고**, 고쳐지면 시험이 깨져 이름을 뒤집게 한다.
//
// 기계적 원인은 하나다. termwiz 의 `clear_eol` · `clear_eos` 가 지울 범위를
// `lines[ypos].fill_range(xpos..width)` 로 정하는데, 걸친 커서는 `xpos == cols` 라 그
// 구간이 **빈 구간**이 된다. 고치는 길은 그 한 칸을 손으로 찍는 것뿐이고, 그러면 그
// 칸만 pen 이 달라진다 — 그래서 소거가 어떤 pen 으로 지우는가를 먼저 정해야 닿는다.

#[test]
fn el0_at_a_parked_cursor_erases_nothing_known_gap() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[0K");
    assert_eq!(
        &t.screen_row(0, true),
        "ABCDEFGHIJ",
        "알려진 결함: 커서가 올라앉은 마지막 칸이 범위에 안 들어 한 칸도 안 지워진다 \
         (규격대로면 \"ABCDEFGHI\")"
    );
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태 자체는 보존된다");

    // 대조군 — 걸치지 않은 마지막 열에서는 그 칸이 제대로 지워진다. 원인이 "EL0 이
    // 아무것도 안 지운다" 가 아니라 **걸침** 임을 가르는 장치다.
    let mut control = at_column("ABCDEFGHIJ", COLS - 1);
    control.feed_bytes(b"\x1b[0K");
    assert_eq!(&control.screen_row(0, true), "ABCDEFGHI");
}

#[test]
fn ed0_at_a_parked_cursor_spares_the_cursor_row_known_gap() {
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[0J");

    assert_eq!(
        &t.screen_row(0, true),
        "ABCDEFGHIJ",
        "알려진 결함: 커서 행은 한 칸도 안 지워진다 (규격대로면 \"ABCDEFGHI\")"
    );
    assert_eq!(
        &t.screen_row(1, true),
        "",
        "아래 행은 정상적으로 지워진다 — 빠지는 것은 커서 행의 마지막 한 칸뿐이다"
    );
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태 자체는 보존된다");
}
