//! VTE handler: edit 도메인.
//!
//! ## 소거 명령과 "걸친 커서"
//!
//! ED/EL 은 **커서를 움직이지 않는 소거 연산**이고, 지우는 범위는 커서가 올라앉은
//! 칸을 **포함**한다. 그 둘이 이 모듈의 모든 갈래가 지켜야 하는 계약이다.
//!
//! 자동 줄바꿈이 대기 중인 상태("행 끝에 걸친 커서")를 이 구현은 `cursor_position()`
//! 의 열이 화면 폭과 **같은 값**(`cx == cols`)인 것으로 나타낸다. 그 칸은 그리드에
//! 없다 — 커서가 실제로 올라앉은 칸은 마지막 열(`cols - 1`)이고, 줄바꿈은 다음
//! 글자를 찍을 때로 미뤄져 있다. 그래서 소거 범위를 셀 때 걸친 커서는 **마지막
//! 열로 친다**: EL1 은 행 전체(`cols` 칸)를, EL0 은 마지막 한 칸을 지운다.
//!
//! **전제 — 위 두 문장 중 첫째("커서를 안 움직인다")가 "걸친 상태를 안 푼다" 를
//! 파생시키는 것은 이 표현 안에서만이다.** 걸침을 담는 자리가 커서 위치뿐이라서
//! 성립한다. termwiz `Surface` 에는 wrap/pending 플래그가 **없고**(필드는
//! `width` · `height` · `lines` · `attributes` · `xpos` · `ypos` · `seqno` ·
//! `changes` · 커서 모양/가시성/색 · `title` 뿐이다), 글자를 찍을 때 `xpos` 를
//! 클램프 없이 전진시킨 뒤 줄바꿈을 **다음 글자로 미루며**, `compute_position_change`
//! 가 `Absolute` 와 `Relative(+)` 를 `limit - 1` 로 자른다. 즉 상태 공간이 정확히
//! `(xpos, ypos)` 다. xterm 계열처럼 걸침을 **별도 플래그**(`do_wrap` / `wrapnext`)로
//! 가지는 표현으로 옮기면 "커서를 안 움직인다" 가 그 플래그의 운명을 정하지 않으므로
//! **이 파생은 무효가 되고**, 걸침 보존을 갈래마다 따로 정해야 한다.
//!
//! 되돌릴 때 주의할 것이 하나 있다. **`Position::Absolute(cols)` 는 `cols - 1` 로
//! 잘린다** — 커서를 옮기는 어떤 `Change` 로도 걸친 자리에 도달할 수 없고(CUP·CUF
//! 도 같다), 그 자리에 닿는 길은 **마지막 열까지 글자를 찍는 것** 하나뿐이다.
//! 그래서 소거 범위를 손으로 찍는 갈래(EL1 · ED1)는 소거를 마지막 열에서 끝내
//! 커서가 스스로 다시 걸치게 하고, `Absolute(cx)` 복원을 내보내지 않는다. 복원을
//! 내보내면 걸친 상태가 풀려 다음 글자가 줄바꿈 없이 마지막 칸을 덮어쓴다.
//!
//! 반대로 termwiz 의 지우기 primitive 로 지우는 갈래(EL2 · ED2)는 소거가 끝난 뒤
//! 커서가 제자리에 없어 복원을 **내보내야** 한다. **그 까닭은 둘이 서로 다르다.**
//! ED2 는 primitive 자신이 옮긴다 — `Surface::clear_screen` 이 채우기를 마치고
//! `xpos = 0; ypos = 0` 을 한다. EL2 는 그렇지 않다 — `Surface::clear_eol` 은
//! `lines[ypos].fill_range(xpos..width)` 만 하고 커서를 **안 건드린다**. EL2 에서
//! 커서가 0 열로 가는 것은 **이 파일의 코드**가 옮긴 것이다: 그 primitive 가 커서
//! 열부터 지우므로 행 전체를 지우려면 먼저 0 열로 가야 한다. (같은 `xpos` 의존이
//! EL0 · ED0 의 범위 결함도 만든다 — 걸친 커서에서는 `xpos == cols` 라 그 범위가
//! 빈 구간이 된다.)
//!
//! 이 두 갈래는 다시 걸치게 할 글자를 찍을 수 없어(찍으면 그 칸만 pen 이 달라진다)
//! 걸친 커서가 마지막 열로 내려앉고 대기 중이던 줄바꿈은 풀린다 — 0 열 · 홈으로
//! 끌려가는 것보다는 가깝지만 완전한 보존은 아니다.
//!
//! 이 계약이 닿지 않는 갈래는 `docs/features/terminal/index.md` 의 "소거 명령과
//! 걸친 커서" 표가 이름으로 센다.

use termwiz::cell::unicode_column_width;
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{Edit, EraseInDisplay, EraseInLine};
use termwiz::surface::{Change, Position};

use crate::TerminalState;

/// 0 열부터 커서 칸까지(커서 포함) 지워야 할 칸 수.
///
/// 걸친 커서(`cx == cols`)는 마지막 열에 올라앉은 것으로 치므로 행 전체(`cols`)이고,
/// 0 열 커서는 **한 칸**이다(0 칸이 아니다 — 커서 칸이 범위에 포함된다).
fn erase_span_to_cursor(cx: usize, cols: usize) -> usize {
    (cx + 1).min(cols)
}

/// 소거를 마친 뒤 커서를 원래 열로 되돌리는 `Change` — 걸쳐 있었으면 **비어 있다**.
///
/// 걸친 자리(`cx == cols`)는 `Position::Absolute` 로 못 가리킨다(`cols - 1` 로 잘린다).
/// 그 대신 [`erase_span_to_cursor`] 가 마지막 열까지 찍어 커서가 스스로 다시 걸치므로,
/// 그 경우에 복원을 내보내면 오히려 걸친 상태를 푼다.
fn restore_cursor_column(cx: usize, cy: usize, cols: usize) -> Option<Change> {
    (cx < cols).then_some(Change::CursorPosition {
        x: Position::Absolute(cx),
        y: Position::Absolute(cy),
    })
}

impl TerminalState {
    pub(crate) fn map_edit(&mut self, edit: Edit) -> Vec<Change> {
        match edit {
            Edit::EraseInDisplay(mode) => match mode {
                EraseInDisplay::EraseToEndOfDisplay => {
                    vec![Change::ClearToEndOfScreen(ColorAttribute::Default)]
                }
                EraseInDisplay::EraseToStartOfDisplay => {
                    let (cx, cy) = self.surface().cursor_position();
                    let (cols, _rows) = self.surface().dimensions();
                    let mut changes = Vec::new();
                    for row in 0..cy {
                        changes.push(Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(row),
                        });
                        changes.push(Change::ClearToEndOfLine(ColorAttribute::Default));
                    }
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    });
                    changes.push(Change::Text(" ".repeat(erase_span_to_cursor(cx, cols))));
                    changes.extend(restore_cursor_column(cx, cy, cols));
                    changes
                }
                EraseInDisplay::EraseDisplay => {
                    let (cx, cy) = self.surface().cursor_position();
                    vec![
                        Change::ClearScreen(ColorAttribute::Default),
                        // `ClearScreen` 은 커서를 홈으로 보낸다 — ED 는 커서를 움직이지
                        // 않으므로 되돌린다. `clear` 류가 보내는 `ESC [ H ESC [ 2J` 는
                        // 먼저 홈으로 가므로 이 복원에 영향을 받지 않는다.
                        Change::CursorPosition {
                            x: Position::Absolute(cx),
                            y: Position::Absolute(cy),
                        },
                    ]
                }
                EraseInDisplay::EraseScrollback => {
                    // ED3: erase scrollback history only — the visible screen is
                    // preserved (no Change emitted). `clear` 같은 명령이 보내는
                    // `\x1b[3J\x1b[2J` 류에서 ED2 가 화면을, ED3 가 스크롤백을 지운다.
                    self.clear_scrollback();
                    vec![]
                }
            },
            Edit::EraseInLine(mode) => match mode {
                EraseInLine::EraseToEndOfLine => {
                    vec![Change::ClearToEndOfLine(ColorAttribute::Default)]
                }
                EraseInLine::EraseToStartOfLine => {
                    let (cx, cy) = self.surface().cursor_position();
                    let (cols, _rows) = self.surface().dimensions();
                    let mut changes = vec![Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    }];
                    changes.push(Change::Text(" ".repeat(erase_span_to_cursor(cx, cols))));
                    changes.extend(restore_cursor_column(cx, cy, cols));
                    changes
                }
                EraseInLine::EraseLine => {
                    let (cx, cy) = self.surface().cursor_position();
                    vec![
                        Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(cy),
                        },
                        Change::ClearToEndOfLine(ColorAttribute::Default),
                        // 0 열로 옮긴 것은 지우기 위한 것이므로 되돌린다 — ED/EL 은
                        // 커서를 움직이지 않는다.
                        Change::CursorPosition {
                            x: Position::Absolute(cx),
                            y: Position::Absolute(cy),
                        },
                    ]
                }
            },
            Edit::ScrollUp(n) => {
                let (first_row, region_size) = self.scroll_region_params();
                vec![Change::ScrollRegionUp {
                    first_row,
                    region_size,
                    scroll_count: n as usize,
                }]
            }
            Edit::ScrollDown(n) => {
                let (first_row, region_size) = self.scroll_region_params();
                vec![Change::ScrollRegionDown {
                    first_row,
                    region_size,
                    scroll_count: n as usize,
                }]
            }
            Edit::DeleteCharacter(n) => {
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let remaining = cols.saturating_sub(cx);
                let n = (n as usize).min(remaining);
                if n == 0 {
                    return vec![];
                }
                let line = self.read_line_from_surface(cy, cx, cols);
                // Skip n columns worth of characters (n is in cells, not chars)
                let mut skip_cols = 0;
                let mut skip_chars = 0;
                for ch in line.chars() {
                    if skip_cols >= n {
                        break;
                    }
                    skip_cols += unicode_column_width(&ch.to_string(), None);
                    skip_chars += 1;
                }
                let after: String = line.chars().skip(skip_chars).collect();
                let after_width: usize = after
                    .chars()
                    .map(|c| unicode_column_width(&c.to_string(), None))
                    .sum();
                let mut text = after;
                for _ in 0..remaining.saturating_sub(after_width) {
                    text.push(' ');
                }
                vec![
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                    Change::Text(text),
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::InsertCharacter(n) => self.insert_blank_changes(n as usize),
            Edit::DeleteLine(n) => {
                let (_cx, cy) = self.surface().cursor_position();
                let (first_row, region_size) = self.scroll_region_params();
                let effective_first = cy.max(first_row);
                let effective_size = (first_row + region_size).saturating_sub(effective_first);
                if effective_size == 0 {
                    return vec![];
                }
                vec![
                    Change::ScrollRegionUp {
                        first_row: effective_first,
                        region_size: effective_size,
                        scroll_count: n as usize,
                    },
                    Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::InsertLine(n) => {
                let (_cx, cy) = self.surface().cursor_position();
                let (first_row, region_size) = self.scroll_region_params();
                let effective_first = cy.max(first_row);
                let effective_size = (first_row + region_size).saturating_sub(effective_first);
                if effective_size == 0 {
                    return vec![];
                }
                vec![
                    Change::ScrollRegionDown {
                        first_row: effective_first,
                        region_size: effective_size,
                        scroll_count: n as usize,
                    },
                    Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::EraseCharacter(n) => {
                let (cx, cy) = self.surface().cursor_position();
                let (cols, _rows) = self.surface().dimensions();
                let n = (n as usize).min(cols.saturating_sub(cx));
                if n == 0 {
                    return vec![];
                }
                vec![
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                    Change::Text(" ".repeat(n)),
                    Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    },
                ]
            }
            Edit::Repeat(n) => {
                // REP (CSI b): repeat the last printed character n times. We track
                // the last printed grapheme in `last_print` (set in
                // action_to_changes). Clamp the count to the grid area to bound
                // allocation against a hostile parameter.
                let Some(ch) = self.last_print.clone() else {
                    return vec![];
                };
                let count = (n.max(1) as usize).min(self.cols.saturating_mul(self.rows));
                let text = ch.repeat(count);
                let width = unicode_column_width(&text, None);
                self.print_with_insert_mode(width, text)
            }
        }
    }

    /// Insert `n` blank columns at the cursor, shifting existing line content to
    /// the right (ICH semantics). The cursor stays at its original position.
    /// Shared by the ICH edit op and by IRM (insert-mode) printing. Returns an
    /// empty change list when there is no room to shift.
    pub(crate) fn insert_blank_changes(&self, n: usize) -> Vec<Change> {
        let (cx, cy) = self.surface().cursor_position();
        let (cols, _rows) = self.surface().dimensions();
        let remaining = cols.saturating_sub(cx);
        let n = n.min(remaining);
        if n == 0 {
            return vec![];
        }
        let line = self.read_line_from_surface(cy, cx, cols);
        // Insert n blank columns, then append existing content that fits
        let mut text = " ".repeat(n);
        let mut used_cols = n;
        for ch in line.chars() {
            let w = unicode_column_width(&ch.to_string(), None);
            if used_cols + w > remaining {
                break;
            }
            text.push(ch);
            used_cols += w;
        }
        while used_cols < remaining {
            text.push(' ');
            used_cols += 1;
        }
        vec![
            Change::CursorPosition {
                x: Position::Absolute(cx),
                y: Position::Absolute(cy),
            },
            Change::Text(text),
            Change::CursorPosition {
                x: Position::Absolute(cx),
                y: Position::Absolute(cy),
            },
        ]
    }
}
