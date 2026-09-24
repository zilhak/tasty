//! `Terminal` 의 screen / cell 접근 메서드.

use termwiz::cell::{CellAttributes, Intensity, Underline};
use termwiz::color::ColorAttribute;
use termwiz::surface::line::Line;

use crate::{CellInfo, TerminalState};

/// dim 속성은 입력 후보에도 쓰이지만 일반적인 흐린 텍스트와 구별할 수는 없다.
fn is_dim(attrs: &CellAttributes) -> bool {
    attrs.intensity() == Intensity::Half
}

/// 셀 텍스트를 이어 붙인다. include_dim=false면 dim 셀을 공백으로 바꾸지 않고 제외한다.
fn line_text(line: &Line, include_dim: bool) -> String {
    let mut text = String::new();
    for cell in line.visible_cells() {
        if !include_dim && is_dim(cell.attrs()) {
            continue;
        }
        text.push_str(cell.str());
    }
    text
}

impl TerminalState {
    /// Get the visible text content of the screen as a string.
    /// Each row is on its own line, trailing spaces are trimmed.
    /// `include_dim=false` excludes dim (ghost-suggestion) cells — the default
    /// used by `surface.screen_text` / `pty.read` so CLI autocomplete overlays
    /// (e.g. Claude Code's ghost text) aren't mistaken for real buffer content.
    pub fn screen_text(&self, include_dim: bool) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        let mut result = String::new();
        for line in lines {
            result.push_str(line_text(&line, include_dim).trim_end());
            result.push('\n');
        }
        // Trim trailing empty lines
        while result.ends_with("\n\n") {
            result.pop();
        }
        result
    }

    /// Return up to n lines ending at the last nonblank screen row, then fill any
    /// shortage from the primary scrollback, including when the alternate screen is active.
    /// Skip only trailing blank screen rows; keep blank lines inside content and scrollback.
    /// Use include_dim for both blank-row detection and returned text, so dim-only rows
    /// may change where the content ends. Return all available lines if fewer than n exist.
    pub fn screen_text_lines(&self, n: usize, include_dim: bool) -> String {
        let surface = self.surface();
        let screen_lines = surface.screen_lines();
        let scrollback_total = self.scrollback_len();

        // Reuse each rendered row for both blank-row detection and output.
        let rendered: Vec<String> = screen_lines
            .iter()
            .map(|line| line_text(line, include_dim).trim_end().to_string())
            .collect();

        // One past the last nonblank rendered row.
        let content_end = rendered
            .iter()
            .rposition(|text| !text.is_empty())
            .map_or(0, |i| i + 1);

        let take_from_screen = n.min(content_end);
        let screen_start = content_end - take_from_screen;
        let scrollback_needed = (n - take_from_screen).min(scrollback_total);
        let scrollback_start = scrollback_total - scrollback_needed;

        let mut result = String::new();

        // Oldest first: the scrollback fill comes before the screen slice.
        for i in scrollback_start..scrollback_total {
            let text = self
                .scrollback_line_owned(i)
                .map(|cells| {
                    cells
                        .iter()
                        .filter(|(_, attrs)| include_dim || !is_dim(attrs))
                        .map(|(s, _)| s.as_str())
                        .collect::<String>()
                })
                .unwrap_or_default();
            result.push_str(text.trim_end());
            result.push('\n');
        }

        for text in &rendered[screen_start..content_end] {
            result.push_str(text);
            result.push('\n');
        }

        result
    }

    /// Get the text of a specific row (0-indexed), trimmed.
    pub fn screen_row(&self, row: usize, include_dim: bool) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return String::new();
        }
        line_text(&lines[row], include_dim).trim_end().to_string()
    }

    /// Get detailed information about a specific cell (row, col) on the current screen.
    /// Returns None if row/col is out of bounds.
    pub fn cell_info(&self, row: usize, col: usize) -> Option<CellInfo> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return None;
        }
        for cell in lines[row].visible_cells() {
            if cell.cell_index() == col {
                let attrs = cell.attrs();
                let width = if cell
                    .str()
                    .chars()
                    .next()
                    .is_some_and(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1)
                {
                    2
                } else {
                    1
                };
                return Some(Self::build_cell_info(cell.str().to_string(), attrs, width));
            }
        }
        None
    }

    /// Get cell info for all cells in a specific row.
    /// Returns empty vec if row is out of bounds.
    pub fn row_cells(&self, row: usize) -> Vec<(usize, CellInfo)> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return Vec::new();
        }
        lines[row]
            .visible_cells()
            .map(|cell| {
                let attrs = cell.attrs();
                let width = if cell
                    .str()
                    .chars()
                    .next()
                    .is_some_and(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1)
                {
                    2
                } else {
                    1
                };
                (
                    cell.cell_index(),
                    Self::build_cell_info(cell.str().to_string(), attrs, width),
                )
            })
            .collect()
    }

    /// Get the raw `CellAttributes` for a cell on the current screen.
    /// Used by callers that need to compute renderer colors (e.g. `debug.glyph_color`).
    pub fn cell_attrs(&self, row: usize, col: usize) -> Option<CellAttributes> {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return None;
        }
        for cell in lines[row].visible_cells() {
            if cell.cell_index() == col {
                return Some(cell.attrs().clone());
            }
        }
        None
    }

    pub(crate) fn build_cell_info(text: String, attrs: &CellAttributes, width: usize) -> CellInfo {
        let intensity = match attrs.intensity() {
            termwiz::cell::Intensity::Normal => "normal",
            termwiz::cell::Intensity::Bold => "bold",
            termwiz::cell::Intensity::Half => "half",
        };
        let underline_style = match attrs.underline() {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Double => "double",
            Underline::Curly => "curly",
            Underline::Dotted => "dotted",
            Underline::Dashed => "dashed",
        };
        let blink = match attrs.blink() {
            termwiz::cell::Blink::None => "none",
            termwiz::cell::Blink::Slow => "slow",
            termwiz::cell::Blink::Rapid => "rapid",
        };
        let vertical_align = match attrs.vertical_align() {
            termwiz::cell::VerticalAlign::BaseLine => "baseline",
            termwiz::cell::VerticalAlign::SuperScript => "super",
            termwiz::cell::VerticalAlign::SubScript => "sub",
        };
        CellInfo {
            text,
            fg: Self::color_attr_to_string(&attrs.foreground()),
            bg: Self::color_attr_to_string(&attrs.background()),
            bold: attrs.intensity() == termwiz::cell::Intensity::Bold,
            italic: attrs.italic(),
            underline: attrs.underline() != Underline::None,
            strikethrough: attrs.strikethrough(),
            inverse: attrs.reverse(),
            width,
            intensity,
            underline_style,
            underline_color: Self::color_attr_to_string(&attrs.underline_color()),
            blink,
            invisible: attrs.invisible(),
            overline: attrs.overline(),
            vertical_align,
        }
    }

    pub(crate) fn color_attr_to_string(attr: &ColorAttribute) -> String {
        match attr {
            ColorAttribute::Default => "default".to_string(),
            ColorAttribute::PaletteIndex(idx) => format!("palette:{idx}"),
            ColorAttribute::TrueColorWithPaletteFallback(srgba, _)
            | ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
                format!(
                    "#{:02x}{:02x}{:02x}",
                    (srgba.0 * 255.0) as u8,
                    (srgba.1 * 255.0) as u8,
                    (srgba.2 * 255.0) as u8
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Terminal;

    /// SGR 2 marks dim text, including the input suggestions represented by these fixtures.
    const SGR_FAINT: &[u8] = b"\x1b[2m";
    const SGR_RESET: &[u8] = b"\x1b[0m";

    #[test]
    fn screen_row_excludes_dim_by_default() {
        let mut t = Terminal::new_detached(20, 1);
        t.feed_bytes(b"real ");
        t.feed_bytes(SGR_FAINT);
        t.feed_bytes(b"ghost");
        t.feed_bytes(SGR_RESET);

        assert_eq!(t.screen_row(0, false), "real");
        assert_eq!(t.screen_row(0, true), "real ghost");
    }

    #[test]
    fn screen_text_excludes_dim_by_default() {
        let mut t = Terminal::new_detached(20, 1);
        t.feed_bytes(b"real ");
        t.feed_bytes(SGR_FAINT);
        t.feed_bytes(b"ghost");
        t.feed_bytes(SGR_RESET);

        assert_eq!(t.screen_text(false).trim_end(), "real");
        assert!(t.screen_text(true).contains("ghost"));
    }

    #[test]
    fn screen_text_lines_excludes_dim_by_default() {
        let mut t = Terminal::new_detached(20, 1);
        t.feed_bytes(b"real ");
        t.feed_bytes(SGR_FAINT);
        t.feed_bytes(b"ghost");
        t.feed_bytes(SGR_RESET);

        assert_eq!(t.screen_text_lines(1, false).trim_end(), "real");
        assert!(t.screen_text_lines(1, true).contains("ghost"));
    }

    /// 화면 아래의 빈 행을 제외하고 마지막 내용부터 센다. PTY 없이 검사한다.
    #[test]
    fn screen_text_lines_skips_trailing_blank_rows() {
        let mut t = Terminal::new_detached(20, 24);
        t.feed_bytes(b"one\r\ntwo\r\nthree");

        assert_eq!(t.screen_text_lines(6, false), "one\ntwo\nthree\n");
    }

    /// 내용이 `n` 보다 많으면 정확히 마지막 `n` 줄만 나온다.
    #[test]
    fn screen_text_lines_returns_exactly_last_n_of_content() {
        let mut t = Terminal::new_detached(20, 24);
        let payload: Vec<u8> = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\r\n")
            .into_bytes();
        t.feed_bytes(&payload);

        assert_eq!(t.screen_text_lines(3, false), "line7\nline8\nline9\n");
    }

    /// 화면 내용이 `n` 에 모자라면 스크롤백에서 채운다 — 순서는 [스크롤백] → [화면].
    #[test]
    fn screen_text_lines_fills_shortfall_from_scrollback() {
        // 4 행짜리 화면에 8 줄을 흘리면 앞 4 줄이 스크롤백으로 밀리고 뒤 4 줄이 남는다.
        let mut t = Terminal::new_detached(20, 4);
        t.feed_bytes(b"s0\r\ns1\r\ns2\r\ns3\r\ns4\r\nv0\r\nv1\r\nv2");
        assert_eq!(t.scrollback_len(), 4, "앞 4 줄이 스크롤백으로 밀려야 한다");

        // 화면이 줄 수 있는 건 4 줄뿐 → 부족한 2 줄을 스크롤백 *끝* 에서 가져온다.
        assert_eq!(t.screen_text_lines(6, false), "s2\ns3\ns4\nv0\nv1\nv2\n");
    }

    /// 하단 빈 행을 제외한 뒤 부족한 줄을 스크롤백에서 채운다.
    #[test]
    fn screen_text_lines_fills_from_scrollback_when_screen_has_trailing_blanks() {
        let mut t = Terminal::new_detached(20, 6);
        // 6 행 화면에 8 줄 → 앞 2 줄이 스크롤백, 화면엔 6 줄.
        t.feed_bytes(b"p0\r\np1\r\np2\r\np3\r\np4\r\np5\r\np6\r\np7");
        assert_eq!(t.scrollback_len(), 2);
        // 화면을 지우고 위쪽 2 줄만 다시 그려 하단 4 행을 공백으로 만든다.
        t.feed_bytes(b"\x1b[2J\x1b[H");
        t.feed_bytes(b"q0\r\nq1");

        assert_eq!(t.screen_text_lines(4, false), "p0\np1\nq0\nq1\n");
    }

    /// 요청 `n` 이 화면 높이보다 큰 경우에도 스크롤백까지 내려간다.
    #[test]
    fn screen_text_lines_larger_than_screen_height_dips_into_scrollback() {
        let mut t = Terminal::new_detached(20, 4);
        let payload: Vec<u8> = (0..30)
            .map(|i| format!("n{i}"))
            .collect::<Vec<_>>()
            .join("\r\n")
            .into_bytes();
        t.feed_bytes(&payload);

        let out = t.screen_text_lines(10, false);
        let lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 10, "화면 높이(4)를 넘어 10 줄을 채워야 한다");
        assert_eq!(lines.first(), Some(&"n20"));
        assert_eq!(lines.last(), Some(&"n29"));
    }

    /// 스크롤백까지 합쳐도 `n` 에 모자라면 있는 만큼 — 패닉 없음(`min` 방어).
    #[test]
    fn screen_text_lines_returns_all_available_when_short() {
        let mut t = Terminal::new_detached(20, 24);
        t.feed_bytes(b"a\r\nb\r\nc\r\nd");

        assert_eq!(t.screen_text_lines(10, false), "a\nb\nc\nd\n");
    }

    /// 내용 *중간* 의 빈 줄은 실제 출력이므로 보존하고 줄 수에도 포함한다.
    #[test]
    fn screen_text_lines_preserves_interior_blank_lines() {
        let mut t = Terminal::new_detached(20, 24);
        t.feed_bytes(b"a\r\n\r\nb");

        assert_eq!(t.screen_text_lines(3, false), "a\n\nb\n");
    }

    /// 화면이 통째로 비었으면 전부 스크롤백에서 채운다.
    #[test]
    fn screen_text_lines_all_blank_screen_falls_back_to_scrollback() {
        let mut t = Terminal::new_detached(20, 4);
        t.feed_bytes(b"k0\r\nk1\r\nk2\r\nk3\r\nk4\r\n");
        // 마지막 개행으로 커서가 빈 하단 행에 있고, 화면 위쪽만 내용이 있다.
        // 화면을 통째로 지워 content_end == 0 을 만든다(ED2 + 커서 홈).
        t.feed_bytes(b"\x1b[2J\x1b[H");

        let out = t.screen_text_lines(2, false);
        assert!(
            !out.is_empty(),
            "화면이 비어도 스크롤백에서 채워야 한다 (got {out:?})"
        );
        let lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 2);
    }

    /// 화면도 스크롤백도 비어 있으면 패닉 없이 빈 결과.
    #[test]
    fn screen_text_lines_empty_everywhere_is_empty() {
        let t = Terminal::new_detached(20, 24);

        assert_eq!(t.scrollback_len(), 0);
        assert_eq!(t.screen_text_lines(5, false), "");
    }

    /// `include_dim` 값이 빈 행 판정과 출력 양쪽에 같은 값으로 쓰여야 한다 —
    /// ghost 만 있는 행이 `false` 에서는 빈 행, `true` 에서는 내용 행이 된다.
    #[test]
    fn screen_text_lines_blankness_follows_include_dim() {
        let mut t = Terminal::new_detached(20, 24);
        t.feed_bytes(b"real\r\n");
        t.feed_bytes(SGR_FAINT);
        t.feed_bytes(b"ghost");
        t.feed_bytes(SGR_RESET);

        // include_dim=false: ghost 행은 빈 행 → 내용의 끝은 "real".
        assert_eq!(t.screen_text_lines(1, false), "real\n");
        // include_dim=true: ghost 행이 내용의 끝.
        assert_eq!(t.screen_text_lines(1, true), "ghost\n");
    }

    /// 경계값: `n = 0` / `1` / 화면 높이 / 화면 높이 + 1 에서 off-by-one 없음.
    #[test]
    fn screen_text_lines_boundary_values() {
        let mut t = Terminal::new_detached(20, 4);
        // 화면 4 행을 내용으로 가득 채운다(스크롤백 없음).
        t.feed_bytes(b"b0\r\nb1\r\nb2\r\nb3");
        assert_eq!(t.scrollback_len(), 0);

        assert_eq!(t.screen_text_lines(0, false), "");
        assert_eq!(t.screen_text_lines(1, false), "b3\n");
        assert_eq!(t.screen_text_lines(4, false), "b0\nb1\nb2\nb3\n");
        // 스크롤백이 없으므로 화면 높이 + 1 은 화면 전체와 같다.
        assert_eq!(t.screen_text_lines(5, false), "b0\nb1\nb2\nb3\n");
    }

    /// 화면이 가득 찬 alternate screen도 마지막 내용부터 센다.
    #[test]
    fn screen_text_lines_full_screen_is_plain_bottom_slice() {
        let mut t = Terminal::new_detached(20, 4);
        t.feed_bytes(b"\x1b[?1049h"); // alternate screen 진입
        t.feed_bytes(b"t0\r\nt1\r\nt2\r\nt3");

        assert_eq!(t.screen_text_lines(3, false), "t1\nt2\nt3\n");
    }

    /// 화면만 있을 때와 스크롤백도 있을 때의 반환 가능한 줄 수를 비교한다.
    #[test]
    fn saturation_happens_only_when_nothing_is_left() {
        // ① 스크롤백 0 — 화면 내용이 전부다.
        let mut empty = Terminal::new_detached(20, 4);
        empty.feed_bytes(b"\x1b[?1049h");
        empty.feed_bytes(b"t0\r\nt1\r\nt2\r\nt3");
        assert_eq!(empty.scrollback_len(), 0, "전제: 스크롤백이 비어 있다");
        assert_eq!(
            empty.screen_text_lines(200, false).lines().count(),
            4,
            "가진 것이 4 줄이면 200 을 줘도 4 줄이다 — 잘린 것이 아니다"
        );

        // 같은 화면에 스크롤백이 있으면 더 많은 줄을 반환한다.
        let mut filled = Terminal::new_detached(20, 4);
        filled.feed_bytes(b"p0\r\np1\r\np2\r\np3\r\np4\r\np5\r\np6\r\np7");
        filled.feed_bytes(b"\x1b[?1049h");
        filled.feed_bytes(b"t0\r\nt1\r\nt2\r\nt3");
        assert_eq!(filled.scrollback_len(), 4, "전제: 스크롤백 4 줄");
        assert_eq!(
            filled.screen_text_lines(200, false).lines().count(),
            8,
            "화면 4 + 스크롤백 4 — 뷰포트에서 멈추지 않는다"
        );
    }

    /// alternate screen의 줄 수가 부족하면 primary 스크롤백에서 채운다.
    #[test]
    fn screen_text_lines_on_alt_screen_fills_from_primary_scrollback() {
        let mut t = Terminal::new_detached(20, 4);
        // primary 에 8 줄 → 화면 4 줄, 스크롤백 4 줄.
        t.feed_bytes(b"p0\r\np1\r\np2\r\np3\r\np4\r\np5\r\np6\r\np7");
        assert_eq!(t.scrollback_len(), 4, "전제: primary 스크롤백이 4 줄");

        t.feed_bytes(b"\x1b[?1049h"); // alt 스크린 진입 — 자체 스크롤백이 없다
        t.feed_bytes(b"t0\r\nt1\r\nt2\r\nt3");

        // 화면 4 줄로는 6 을 못 채우므로 primary 스크롤백에서 2 줄을 끌어와야 한다.
        assert_eq!(
            t.screen_text_lines(6, false),
            "p2\np3\nt0\nt1\nt2\nt3\n",
            "alt 스크린에서 화면 높이를 넘는 요청이 primary 스크롤백까지 내려가야 한다"
        );
    }

    #[test]
    fn bold_is_not_treated_as_dim() {
        let mut t = Terminal::new_detached(20, 1);
        t.feed_bytes(b"\x1b[1mbold\x1b[0m");

        assert_eq!(t.screen_row(0, false), "bold");
    }
}
