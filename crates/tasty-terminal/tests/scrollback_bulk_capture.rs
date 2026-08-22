//! 스크롤백 벌크 캡처(`Terminal::scrollback_lines_all`)의 동등성 검증.
//!
//! 닫은 항목 스냅샷(`ClosedSurface::from_surface_id_with_restore`)과 layout
//! 영속화(`layout_persistence::scrollback`)는 라인당 `scrollback_line_full` 대신
//! 이 벌크 경로로 스크롤백을 회수한다. 벌크화와 함께 `scrollback_line_full` 자체도
//! `line_owned`(cell 마다 `String` 재할당) + `line_wrapped`(같은 인덱스 재조회) 조합
//! 대신 저장 표현인 `ScrollbackLine` 을 그대로 돌려주도록 바뀌었다.
//!
//! 두 변경 모두 **표현이 보존될 때만** 정당하다. 캡처 결과가 selection / search /
//! link 가 보는 `line_owned` / `line_wrapped` 와 어긋나면 복원된 스크롤백의 내용,
//! cell 속성, wrap 이음새가 원본과 달라진다. 여기서 그 등가를 고정한다.

use tasty_terminal::Terminal;
use termwiz::cell::CellAttributes;

/// 스크롤백을 넉넉히 넘기도록 화면은 좁게 잡는다.
const COLS: usize = 20;
const ROWS: usize = 4;

/// SGR 속성 + auto-wrap + CJK 를 섞어 스크롤백을 채운다 — 단색 ASCII 만으로는
/// RLE 런이 라인당 1개뿐이라 속성 보존이 사실상 검증되지 않는다.
fn feed_mixed(t: &mut Terminal, rounds: usize) {
    for i in 0..rounds {
        t.feed_bytes(format!("plain{i:03}\r\n").as_bytes());
        t.feed_bytes(b"\x1b[1;31mbold-red\x1b[0m normal\r\n");
        t.feed_bytes(b"\x1b[4;32munderline-green\x1b[0m\r\n");
        // COLS 를 넘겨 auto-wrap 을 유발한다 → wrapped=true 라인이 생긴다.
        t.feed_bytes(format!("W{i:03}").as_bytes());
        t.feed_bytes(b"0123456789012345678901234567890123456789\r\n");
        t.feed_bytes("\x1b[7m한글한글한글\x1b[0m\r\n".as_bytes());
    }
}

/// 라인당 경로(`scrollback_line_full`)로 회수한 전량.
fn per_line(t: &Terminal) -> Vec<tasty_terminal::ScrollbackLine> {
    (0..t.scrollback_len())
        .filter_map(|i| t.scrollback_line_full(i))
        .collect()
}

/// selection / search / link 가 쓰는 **변경되지 않은** 접근자로 회수한 기대값.
/// `line_owned` / `line_wrapped` 는 이번 최적화가 건드리지 않았으므로, 캡처
/// 결과를 여기에 맞추는 것이 곧 "표현이 보존됐다" 는 뜻이다.
fn legacy(t: &Terminal) -> Vec<(Vec<(String, CellAttributes)>, bool)> {
    (0..t.scrollback_len())
        .map(|i| {
            (
                t.scrollback_line_owned(i).unwrap_or_default(),
                t.scrollback_line_wrapped(i).unwrap_or(false),
            )
        })
        .collect()
}

/// `ScrollbackLine` 을 비교 가능한 형태로 편다 — cells(그래핌 + 속성) + wrapped.
fn explode(l: &tasty_terminal::ScrollbackLine) -> (Vec<(String, CellAttributes)>, bool) {
    (l.to_cells(), l.wrapped)
}

fn assert_same(
    label: &str,
    got: &[tasty_terminal::ScrollbackLine],
    want_cells: &[(Vec<(String, CellAttributes)>, bool)],
) {
    assert_eq!(
        got.len(),
        want_cells.len(),
        "{label}: 라인 수가 다르다 (got={}, want={})",
        got.len(),
        want_cells.len()
    );
    for (i, (line, want)) in got.iter().zip(want_cells).enumerate() {
        let (cells, wrapped) = explode(line);
        assert_eq!(
            wrapped, want.1,
            "{label}: line {i} 의 wrapped 플래그가 다르다"
        );
        assert_eq!(
            cells.len(),
            want.0.len(),
            "{label}: line {i} 의 cell 수가 다르다"
        );
        for (col, (cell, want_cell)) in cells.iter().zip(&want.0).enumerate() {
            assert_eq!(
                cell.0, want_cell.0,
                "{label}: line {i} col {col} 의 그래핌이 다르다"
            );
            assert_eq!(
                cell.1, want_cell.1,
                "{label}: line {i} col {col} 의 cell 속성이 다르다"
            );
        }
    }
}

/// 벌크 캡처가 라인당 캡처와 완전히 같은 결과를 낸다 (메모리 스크롤백).
#[test]
fn bulk_capture_matches_per_line_capture() {
    let mut t = Terminal::new_detached(COLS, ROWS);
    t.set_scrollback_limit(100_000);
    feed_mixed(&mut t, 40);
    assert!(
        t.scrollback_len() > 100,
        "스크롤백이 충분히 쌓여야 의미 있는 비교다 (len={})",
        t.scrollback_len()
    );

    let bulk = t.scrollback_lines_all();
    let want: Vec<_> = per_line(&t).iter().map(explode).collect();
    assert_same("memory", &bulk, &want);
}

/// 캡처 결과가 selection / search / link 가 쓰는 `line_owned` + `line_wrapped`
/// 와 셀 단위로 일치한다 — 캡처 표현 변경이 복원 내용을 바꾸지 않았음을 고정한다.
#[test]
fn bulk_capture_preserves_cells_wrapped_and_attrs() {
    let mut t = Terminal::new_detached(COLS, ROWS);
    t.set_scrollback_limit(100_000);
    feed_mixed(&mut t, 40);

    let bulk = t.scrollback_lines_all();
    assert_same("legacy-accessors", &bulk, &legacy(&t));

    // 속성이 실제로 섞여 있어야 위 비교가 의미를 가진다 — 전부 default 면
    // RLE 런이 라인당 1개라 속성 회귀를 잡지 못한다.
    let distinct_attrs = bulk
        .iter()
        .flat_map(|l| l.to_cells())
        .filter(|(_, a)| a != &CellAttributes::default())
        .count();
    assert!(
        distinct_attrs > 0,
        "테스트 입력에 비-default cell 속성이 하나도 없다 — 속성 보존이 검증되지 않는다"
    );
    assert!(
        bulk.iter().any(|l| l.wrapped),
        "테스트 입력에 wrapped 라인이 없다 — wrap 플래그 보존이 검증되지 않는다"
    );
}

/// 디스크 스왑이 켜진 스크롤백에서도 등가다. 이 경로는 이전에 `line_owned` /
/// `line_wrapped` 가 같은 인덱스를 독립적으로 읽어 라인당 `File::open` 이 2회
/// 발생했다 — 이제 1회다. 결과가 같아야 그 축소가 정당하다.
#[test]
fn bulk_capture_matches_per_line_on_disk_backed_scrollback() {
    let mut t = Terminal::new_detached(COLS, ROWS);
    // 실행 중 인스턴스의 파일과 겹치지 않도록 테스트 전용 surface id 를 쓴다
    // (경로는 `temp_dir()/tasty-scrollback-debug/surface-<id>.scrollback`).
    t.enable_disk_scrollback(990_017);
    // 메모리 상한을 낮게 잡아 초과분이 디스크로 밀려나게 한다.
    t.set_scrollback_limit(32);
    feed_mixed(&mut t, 40);

    let total = t.scrollback_len();
    assert!(
        total > 32,
        "상한(32) 을 넘겨야 디스크 영역이 생긴다 (len={total})"
    );

    let bulk = t.scrollback_lines_all();
    // 라인당 경로와의 등가(벌크화 검증) + legacy 접근자와의 등가(표현 보존 검증).
    // 후자가 디스크 이중 읽기를 단일 읽기로 줄인 변경의 실제 방어선이다.
    let per = per_line(&t).iter().map(explode).collect::<Vec<_>>();
    assert_same("disk-bulk-vs-perline", &bulk, &per);
    assert_same("disk-legacy-accessors", &bulk, &legacy(&t));
}
