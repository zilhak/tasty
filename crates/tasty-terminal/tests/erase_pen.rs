//! 소거가 **어떤 pen 으로 지우는가**, 그리고 소거가 **pen 을 건드리는가** — 여섯 분기.
//!
//! 두 물음이 하나의 규칙으로 닫힌다.
//!
//! - 지운 칸은 **기본 속성 + 소거 시점의 배경색**을 갖는다(back color erase). 배경색
//!   하나만 옮긴다 — 밑줄·역상 같은 나머지 속성은 지워진 칸에 안 남는다.
//! - 소거가 끝난 뒤 pen 은 **소거 전 그대로**다. 소거 명령은 렌더링 속성을 바꾸지 않는다.
//!
//! **두 값은 따로 어긋날 수 있다.** 한때 `CSI 1K` 는 지운 칸을 배경색으로 채우면서
//! pen 도 그대로 뒀고, `CSI 0K` 는 지운 칸을 기본 배경으로 채우면서 pen 까지 기본으로
//! 되돌렸다. 앞의 값만 재면 뒤가 조용히 어긋나고, 그 어긋남이 더 자주 보인다 —
//! `SGR → EL0 → 텍스트` 는 ncurses 류가 흔히 내는 순서다. 그래서 이 파일은 각 분기에서
//! **지운 칸의 배경**과 **소거 직후 찍은 글자의 배경**을 따로 단언한다.
//!
//! 결정과 그 근거(왜 BCE 인가, 어느 구현을 따랐는가)는
//! `docs/adr/0292-erase-fills-with-the-current-background.md`.
//!
//! ## 측정 장치
//!
//! 화면은 **기본 배경**으로 채우고 배경색은 **소거 직전에만** 켠다. 그래서 소거가
//! 끝난 뒤 배경이 빨강인 칸이 곧 "이 소거가 지운 칸" 이고, 나머지는 안 지워진 칸이다.
//! 두 집합을 따로 단언하므로 "다 빨강이라 통과" 도 "하나도 안 지워서 통과" 도 안 된다.
//!
//! 범위(어느 칸이 지워지는가) 자체는 `erase_boundaries.rs` 가 잰다. 여기서 범위를 또
//! 세는 것은 그 범위가 pen 판정의 **모수**이기 때문이다.

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

/// 커서가 `(4, 1)` 인 배치의 여섯 분기.
///
/// 커서 행을 1 로 두는 것이 요점이다 — **위에 행이 있는 배치**에서 한때 ED1 이 다른
/// 배경으로 지웠다(위 행들을 지우는 primitive 가 pen 을 되돌린 뒤 커서 행을 찍었다).
/// 커서 행 0 만 재면 그 갈래가 안 보인다.
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

/// 소거가 옮기는 것은 **배경색뿐**이다 — 밑줄 같은 나머지 속성은 지운 칸에 안 남는다.
///
/// 이것이 "지운 칸에 현재 pen 을 통째로 찍는다" 와 갈리는 자리다. 두 규칙은 배경만 켠
/// 상태에서는 **같은 값을 낸다** — 그래서 위의 표만으로는 안 갈린다. 독립 구현(tmux 3.4)
/// 에서 `CSI 4;41m` + `CSI 1K` 를 실측해 지운 칸에 밑줄이 없음을 확인했다.
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

/// 음성 대조 — 배경을 안 켜면 지운 칸은 기본 배경이다.
///
/// 이것이 없으면 위 시험들은 "무조건 빨강으로 칠한다" 는 구현으로도 통과한다.
///
/// 커서를 0 열에 두어 각 분기가 **커서 행에서 지우는 칸**을 명시한다 — EL1 · ED1 은
/// 그 한 칸뿐이고 나머지 넷은 행 전체다. 범위 밖 칸은 원래 찍힌 빨강이 그대로 남으므로
/// 여기서 세면 안 된다(그 칸들의 범위는 `erase_boundaries.rs` 가 잰다).
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
