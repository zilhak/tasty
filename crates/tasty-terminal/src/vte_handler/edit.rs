//! VTE handler: edit 도메인.

use termwiz::cell::unicode_column_width;
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{Edit, EraseInDisplay, EraseInLine};
use termwiz::surface::{Change, Position};

use crate::TerminalState;

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
                    if cx < cols {
                        changes.push(Change::Text(" ".repeat(cx + 1)));
                    }
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    });
                    changes
                }
                EraseInDisplay::EraseDisplay => {
                    vec![Change::ClearScreen(ColorAttribute::Default)]
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
                    let mut changes = Vec::new();
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(cy),
                    });
                    if cx > 0 {
                        changes.push(Change::Text(" ".repeat(cx + 1)));
                    }
                    changes.push(Change::CursorPosition {
                        x: Position::Absolute(cx),
                        y: Position::Absolute(cy),
                    });
                    changes
                }
                EraseInLine::EraseLine => {
                    let (_cx, cy) = self.surface().cursor_position();
                    vec![
                        Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(cy),
                        },
                        Change::ClearToEndOfLine(ColorAttribute::Default),
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
            Edit::Repeat(_n) => {
                // REP (CSI b) — 마지막 문자 반복. 우리는 termwiz Surface 위에서
                // 합성하는 구조라 "마지막 문자" 컨텍스트가 없어 미구현.
                vec![]
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
