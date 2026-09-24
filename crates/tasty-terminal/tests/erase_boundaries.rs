//! ED/EL이 커서 칸까지 지우고 커서 위치와 대기 중인 줄바꿈을 보존하는지 확인한다.
//! 0열도 한 칸을 지우며, cx == cols는 마지막 열로 취급한다.
//! 소거 직후 다음 글자가 놓이는 위치와 옆 행 보존도 검사한다.
//! 스크롤 영역이 있는 경우는 region_autowrap.rs에서 확인한다.

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
    // 다음 행의 표지로 소거가 한 칸 넘쳤는지 확인한다. 빈 행은 우측 trim이 공백을 숨긴다.
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

#[test]
fn el1_at_a_parked_cursor_after_wide_glyphs_behaves_the_same() {
    // 전각 5글자가 10열을 채운 상태에서 소거와 다음 글자의 줄바꿈을 검사한다.
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
    // 이 ingest 경로는 e + 결합 악센트를 두 칸으로 배치한다.
    // 문자 결합 방식과 별도로 커서 열까지의 소거 범위를 검사한다.
    let mut t = term();
    t.feed_bytes("e\u{0301}abc".as_bytes());
    let after_text = t.cursor_position();
    assert_eq!(after_text.0, 5, "준비 실패: 이 경로의 셀 배치가 바뀌었다");

    t.feed_bytes(b"\x1b[1;3H\x1b[1K"); // 커서를 2 열로 → 0..=2 세 칸
    assert_eq!(&t.screen_row(0, true), "   bc");
    assert_eq!(t.cursor_position(), (2, 0));
}

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

// EL2는 여기서 0열로 옮겨 지우고 ED2는 termwiz가 홈으로 옮긴다. 둘 다 원래 커서를 복원해야 한다.

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
fn el2_at_a_parked_cursor_keeps_the_pending_wrap() {
    // 마지막 열을 같은 소거 속성으로 다시 찍어 pending wrap을 보존한다.
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[2K");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(
        &t.screen_row(1, true),
        "QRSTUVWXYZ",
        "소거가 한 칸도 다음 행으로 넘기지 않았다"
    );
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태가 보존된다");
}

#[test]
fn el2_at_a_parked_cursor_keeps_the_pending_wrap_for_the_next_glyph() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[2K");
    t.feed_bytes(b"Z");
    assert_eq!(
        (t.screen_row(0, true), t.screen_row(1, true)),
        (String::new(), "Z".to_string()),
        "대기 중이던 줄바꿈이 다음 글자에서 일어난다 — 마지막 칸을 덮어쓰지 않는다"
    );
    assert_eq!(t.cursor_position(), (1, 1));
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
fn ed2_at_a_parked_cursor_keeps_the_pending_wrap() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[2J");
    assert_eq!(&t.screen_row(0, true), "");
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태가 보존된다");
    t.feed_bytes(b"Z");
    assert_eq!(
        (t.screen_row(0, true), t.screen_row(1, true)),
        (String::new(), "Z".to_string()),
        "대기 중이던 줄바꿈이 다음 글자에서 일어난다"
    );
}

/// alternate screen은 스크롤백을 기록하지 않으므로, 스크롤 유무를 화면 내용과 커서로 확인한다.
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

// EL0/ED0은 pending wrap에서도 마지막 한 칸을 지운다. termwiz의 xpos..width는
// 빈 범위가 되므로 그 칸을 직접 처리한다. 이 선택은 tmux 3.4의 관측 동작과 다르며
// docs/features/terminal/index.md#스크롤-영역과-소거에 설명한다.

#[test]
fn el0_at_a_parked_cursor_erases_the_cell_it_sits_on() {
    // 다음 행의 표지로 소거가 한 칸 넘쳤는지 확인한다.
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[0K");

    assert_eq!(
        &t.screen_row(0, true),
        "ABCDEFGHI",
        "커서가 올라앉은 마지막 칸 하나가 지워진다"
    );
    assert_eq!(
        &t.screen_row(1, true),
        "QRSTUVWXYZ",
        "한 칸도 다음 행으로 넘어가지 않는다"
    );
    assert_eq!(t.scrollback_len(), 0, "소거가 화면을 스크롤하지 않는다");
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태가 보존된다");

    // pending wrap이 아닌 마지막 열도 같은 칸을 지워야 한다.
    let mut control = at_column("ABCDEFGHIJ", COLS - 1);
    control.feed_bytes(b"\x1b[0K");
    assert_eq!(&control.screen_row(0, true), "ABCDEFGHI");
}

#[test]
fn el0_at_a_parked_cursor_keeps_the_pending_wrap() {
    let mut t = at_column("ABCDEFGHIJ", COLS);
    t.feed_bytes(b"\x1b[0K");
    t.feed_bytes(b"Z");
    assert_eq!(
        (t.screen_row(0, true), t.screen_row(1, true)),
        ("ABCDEFGHI".to_string(), "Z".to_string()),
        "대기 중이던 줄바꿈이 다음 글자에서 일어난다 — 마지막 칸을 덮어쓰지 않는다"
    );
    assert_eq!(t.cursor_position(), (1, 1));
}

#[test]
fn ed0_at_a_parked_cursor_erases_that_cell_and_every_row_below() {
    let mut t = term();
    t.feed_bytes(b"\x1b[2;1HQRSTUVWXYZ\x1b[3;1HLLLLL\x1b[1;1H");
    t.feed_bytes(b"ABCDEFGHIJ");
    assert_eq!(
        t.cursor_position(),
        (COLS, 0),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[0J");

    assert_eq!(
        &t.screen_row(0, true),
        "ABCDEFGHI",
        "커서 행에서는 커서가 올라앉은 마지막 칸만 지워진다"
    );
    assert_eq!(&t.screen_row(1, true), "", "아래 행은 통째로 지워진다");
    assert_eq!(&t.screen_row(2, true), "", "아래 행 전부다");
    assert_eq!(t.cursor_position(), (COLS, 0), "걸친 상태가 보존된다");
}

#[test]
fn ed0_above_the_cursor_row_is_left_alone() {
    // 커서 행 아래만 지운다 — 위 행이 함께 지워지면 ED0 이 ED2 가 된다. 걸침 갈래가
    // 커서 행에 한 칸을 **찍는** 형태라, 그 찍기가 엉뚱한 행에 가지 않는지도 함께 본다.
    let mut t = term();
    t.feed_bytes(b"\x1b[1;1HAAAA\x1b[3;1HQRSTUVWXYZ\x1b[2;1HBBBBBBBBBB");
    assert_eq!(
        t.cursor_position(),
        (COLS, 1),
        "준비 실패: 커서가 걸쳐야 한다"
    );

    t.feed_bytes(b"\x1b[0J");

    assert_eq!(&t.screen_row(0, true), "AAAA", "위 행은 안 건드린다");
    assert_eq!(&t.screen_row(1, true), "BBBBBBBBB", "커서 칸 하나만");
    assert_eq!(&t.screen_row(2, true), "", "아래 행은 지워진다");
    assert_eq!(t.cursor_position(), (COLS, 1));
}
