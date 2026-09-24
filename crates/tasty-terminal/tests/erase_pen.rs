//! 여섯 ED/EL 분기에서 지운 셀과 이후 출력의 속성을 각각 확인한다.
//! 지운 셀은 기본 속성과 현재 배경색을 갖고, 이후 출력은 소거 전 pen을 유지해야 한다.
//! 화면을 그린 뒤 소거 직전에 배경을 바꿔 지운 범위와 남겨 둔 범위를 구별한다.
//! 동작 근거: docs/features/terminal/index.md#스크롤-영역과-소거.

use tasty_terminal::Terminal;

const COLS: usize = 10;
const ROWS: usize = 4;

/// `CSI 41m` 이 칸에 남기는 값 — `CellInfo::bg` 의 표기.
const RED: &str = "palette:1";

fn bg_at(t: &Terminal, row: usize, col: usize) -> String {
    // 기본 속성의 빈 칸은 `visible_cells()` 에 안 나온다 — 그것과 "명시적 기본 배경" 은
    // 이 축에서 같은 뜻이다(둘 다 배경이 안 칠해진 칸이다).
    t.cell_info(row, col)
        .map(|c| c.bg)
        .unwrap_or_else(|| "default".to_string())
}

/// 네 행을 **기본 배경**으로 채우고 커서를 제자리에 둔 뒤, 배경색만 켜 둔 터미널.
///
/// `park` 면 커서 행을 정확히 `COLS` 칸 찍어 커서를 걸치게 한다(그 자리에 닿는 유일한
/// 길이다 — `Position::Absolute(COLS)` 는 `COLS - 1` 로 잘린다).
fn armed(cursor_row: usize, park: bool) -> Terminal {
    let mut t = Terminal::new_detached(COLS, ROWS);
    t.set_scrollback_limit(1000);
    for r in 0..ROWS {
        t.feed_bytes(format!("\x1b[{};1HABCDEFGHIJ", r + 1).as_bytes());
    }
    if park {
        t.feed_bytes(format!("\x1b[{};1HABCDEFGHIJ", cursor_row + 1).as_bytes());
        assert_eq!(
            t.cursor_position(),
            (COLS, cursor_row),
            "준비 실패: {COLS} 칸을 찍으면 커서가 걸쳐야 한다"
        );
    } else {
        t.feed_bytes(format!("\x1b[{};5H", cursor_row + 1).as_bytes());
        assert_eq!(t.cursor_position(), (4, cursor_row), "준비 실패: 커서 열");
    }
    // 화면은 이미 다 찍혔다 — 이 SGR 은 pen 만 바꾼다. 그래서 이 뒤에 빨강이 되는 칸은
    // 전부 소거가 칠한 것이다.
    t.feed_bytes(b"\x1b[41m");
    t
}

/// 한 분기의 기대 — (이름, 시퀀스, 지워질 칸, 안 지워질 칸).
struct Case {
    name: &'static str,
    seq: &'static [u8],
    erased: Vec<(usize, usize)>,
    kept: Vec<(usize, usize)>,
}

fn row_cells(row: usize, cols: std::ops::Range<usize>) -> Vec<(usize, usize)> {
    cols.map(|c| (row, c)).collect()
}

fn whole_rows(rows: std::ops::Range<usize>) -> Vec<(usize, usize)> {
    rows.flat_map(|r| row_cells(r, 0..COLS)).collect()
}

/// 커서 위 행을 지운 뒤 현재 행을 처리하는 ED1 경로도 검사하도록 커서를 (4, 1)에 둔다.
fn cases_at_row_one() -> Vec<Case> {
    let cur = 1;
    vec![
        Case {
            name: "EL0 CSI 0K",
            seq: b"\x1b[0K",
            erased: row_cells(cur, 4..COLS),
            kept: [row_cells(cur, 0..4), whole_rows(0..1), whole_rows(2..ROWS)].concat(),
        },
        Case {
            name: "EL1 CSI 1K",
            seq: b"\x1b[1K",
            erased: row_cells(cur, 0..5),
            kept: [
                row_cells(cur, 5..COLS),
                whole_rows(0..1),
                whole_rows(2..ROWS),
            ]
            .concat(),
        },
        Case {
            name: "EL2 CSI 2K",
            seq: b"\x1b[2K",
            erased: row_cells(cur, 0..COLS),
            kept: [whole_rows(0..1), whole_rows(2..ROWS)].concat(),
        },
        Case {
            name: "ED0 CSI 0J",
            seq: b"\x1b[0J",
            erased: [row_cells(cur, 4..COLS), whole_rows(cur + 1..ROWS)].concat(),
            kept: [row_cells(cur, 0..4), whole_rows(0..cur)].concat(),
        },
        Case {
            name: "ED1 CSI 1J",
            seq: b"\x1b[1J",
            erased: [whole_rows(0..cur), row_cells(cur, 0..5)].concat(),
            kept: [row_cells(cur, 5..COLS), whole_rows(cur + 1..ROWS)].concat(),
        },
        Case {
            name: "ED2 CSI 2J",
            seq: b"\x1b[2J",
            erased: whole_rows(0..ROWS),
            kept: vec![],
        },
    ]
}

/// 같은 여섯 분기를 **걸친 커서**(행 1)에서. 걸침은 범위를 바꾸므로 기대도 다르다.
fn cases_at_a_parked_cursor() -> Vec<Case> {
    let cur = 1;
    vec![
        Case {
            name: "EL0 CSI 0K (parked)",
            seq: b"\x1b[0K",
            erased: row_cells(cur, COLS - 1..COLS),
            kept: [
                row_cells(cur, 0..COLS - 1),
                whole_rows(0..cur),
                whole_rows(cur + 1..ROWS),
            ]
            .concat(),
        },
        Case {
            name: "EL1 CSI 1K (parked)",
            seq: b"\x1b[1K",
            erased: row_cells(cur, 0..COLS),
            kept: [whole_rows(0..cur), whole_rows(cur + 1..ROWS)].concat(),
        },
        Case {
            name: "EL2 CSI 2K (parked)",
            seq: b"\x1b[2K",
            erased: row_cells(cur, 0..COLS),
            kept: [whole_rows(0..cur), whole_rows(cur + 1..ROWS)].concat(),
        },
        Case {
            name: "ED0 CSI 0J (parked)",
            seq: b"\x1b[0J",
            erased: [row_cells(cur, COLS - 1..COLS), whole_rows(cur + 1..ROWS)].concat(),
            kept: [row_cells(cur, 0..COLS - 1), whole_rows(0..cur)].concat(),
        },
        Case {
            name: "ED1 CSI 1J (parked)",
            seq: b"\x1b[1J",
            erased: [whole_rows(0..cur), row_cells(cur, 0..COLS)].concat(),
            kept: whole_rows(cur + 1..ROWS),
        },
        Case {
            name: "ED2 CSI 2J (parked)",
            seq: b"\x1b[2J",
            erased: whole_rows(0..ROWS),
            kept: vec![],
        },
    ]
}

fn check_fill(cases: Vec<Case>, park: bool) {
    for case in cases {
        let mut t = armed(1, park);
        t.feed_bytes(case.seq);
        for (r, c) in &case.erased {
            assert_eq!(
                bg_at(&t, *r, *c),
                RED,
                "{}: 지운 칸 ({r},{c}) 이 소거 시점의 배경색을 안 받았다",
                case.name
            );
        }
        for (r, c) in &case.kept {
            assert_ne!(
                bg_at(&t, *r, *c),
                RED,
                "{}: 안 지워야 할 칸 ({r},{c}) 까지 칠해졌다 — 범위가 넓다",
                case.name
            );
        }
        assert!(
            !case.erased.is_empty(),
            "{}: 지운 칸이 0 개인 기대는 이 시험을 공허하게 만든다",
            case.name
        );
    }
}

#[test]
fn every_erase_branch_fills_with_the_current_background() {
    check_fill(cases_at_row_one(), false);
}

#[test]
fn every_erase_branch_fills_with_the_current_background_at_a_parked_cursor() {
    check_fill(cases_at_a_parked_cursor(), true);
}

/// 소거 **직후 찍는 글자**가 소거 전 pen 을 그대로 받는가 — 위와 다른 값이다.
fn check_pen_survives(park: bool) {
    let seqs: &[(&str, &[u8])] = &[
        ("EL0 CSI 0K", b"\x1b[0K"),
        ("EL1 CSI 1K", b"\x1b[1K"),
        ("EL2 CSI 2K", b"\x1b[2K"),
        ("ED0 CSI 0J", b"\x1b[0J"),
        ("ED1 CSI 1J", b"\x1b[1J"),
        ("ED2 CSI 2J", b"\x1b[2J"),
    ];
    for (name, seq) in seqs {
        let mut t = armed(1, park);
        t.feed_bytes(seq);
        t.feed_bytes(b"Z");
        let (zx, zy) = t.cursor_position();
        let (gr, gc) = (zy, zx - 1);
        assert_eq!(
            t.cell_info(gr, gc).map(|c| c.text),
            Some("Z".to_string()),
            "{name}: 준비 실패 — 찍은 글자를 ({gr},{gc}) 에서 못 찾았다"
        );
        assert_eq!(
            bg_at(&t, gr, gc),
            RED,
            "{name}: 소거가 pen 을 되돌렸다 — 소거 뒤 찍은 글자가 배경색을 잃었다"
        );
    }
}

#[test]
fn every_erase_branch_leaves_the_pen_for_the_next_glyph() {
    check_pen_survives(false);
}

#[test]
fn every_erase_branch_leaves_the_pen_for_the_next_glyph_at_a_parked_cursor() {
    check_pen_survives(true);
}

/// 소거는 배경색만 남긴다. 밑줄까지 켜서 전체 pen 복사와 구별한다.
#[test]
fn erase_carries_the_background_but_not_the_other_attributes() {
    let mut t = Terminal::new_detached(COLS, ROWS);
    t.set_scrollback_limit(1000);
    t.feed_bytes(b"ABCDEFGHIJ");
    t.feed_bytes(b"\x1b[1;5H");
    t.feed_bytes(b"\x1b[4;41m"); // 밑줄 + 빨강 배경
    t.feed_bytes(b"\x1b[0K");

    let erased = t.cell_info(0, 6).expect("지운 칸이 비어 있다");
    assert_eq!(erased.bg, RED, "배경색은 옮겨진다");
    assert_eq!(
        erased.underline_style, "none",
        "밑줄은 지운 칸에 안 남는다 — 소거가 옮기는 것은 배경색뿐이다"
    );

    // 그런데 pen 자체는 온전하다 — 다음 글자는 밑줄도 배경도 갖는다.
    t.feed_bytes(b"Z");
    let glyph = t.cell_info(0, 4).expect("찍은 글자가 없다");
    assert_eq!(glyph.text, "Z", "준비 실패: 글자 자리");
    assert_eq!(glyph.bg, RED);
    assert_eq!(
        glyph.underline_style, "single",
        "소거는 pen 을 안 건드린다 — 밑줄이 살아 있어야 한다"
    );
}

/// 기본 배경으로 지워야 하는 경우도 확인해 무조건 빨강으로 칠하는 구현을 거른다.
/// 커서 0열에서 EL1/ED1은 한 칸, 나머지는 현재 행 전체를 검사한다.
#[test]
fn erase_with_the_default_background_leaves_the_cells_default() {
    let cases: &[(&[u8], std::ops::Range<usize>)] = &[
        (b"\x1b[0K", 0..COLS),
        (b"\x1b[1K", 0..1),
        (b"\x1b[2K", 0..COLS),
        (b"\x1b[0J", 0..COLS),
        (b"\x1b[1J", 0..1),
        (b"\x1b[2J", 0..COLS),
    ];
    for (seq, cols) in cases {
        let mut t = Terminal::new_detached(COLS, ROWS);
        t.set_scrollback_limit(1000);
        t.feed_bytes(b"\x1b[41mABCDEFGHIJ"); // 빨강으로 찍어 두고
        t.feed_bytes(b"\x1b[1;1H\x1b[49m"); // 소거 직전에 배경을 끄고 0 열로
        t.feed_bytes(seq);
        for col in cols.clone() {
            assert_eq!(
                bg_at(&t, 0, col),
                "default",
                "배경이 기본일 때 소거한 칸은 기본이어야 한다 (seq={seq:?}, col={col})"
            );
        }
        // 그리고 소거 뒤 찍는 글자도 기본이다 — pen 복원이 옛 값을 되살리지 않는다.
        t.feed_bytes(b"Z");
        let (zx, zy) = t.cursor_position();
        assert_eq!(
            bg_at(&t, zy, zx - 1),
            "default",
            "pen 복원이 꺼 둔 배경을 되살렸다 (seq={seq:?})"
        );
    }
}
