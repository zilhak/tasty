//! `Terminal` 의 screen / cell 접근 메서드.

use termwiz::cell::{CellAttributes, Intensity, Underline};
use termwiz::color::ColorAttribute;
use termwiz::surface::line::Line;

use crate::{CellInfo, TerminalState};

/// Claude Code 등 CLI 가 그리는 ghost-suggestion(자동완성 제안) 텍스트는 dim
/// (`Intensity::Half`) 속성으로 렌더링된다 — 실제 입력된 텍스트와 구분하는 신호로
/// 검증됨 (실측: 동일 행에서 실제 타이핑은 `Normal`, ghost 제안은 `Half`).
fn is_dim(attrs: &CellAttributes) -> bool {
    attrs.intensity() == Intensity::Half
}

/// 한 행의 텍스트를 셀 단위로 이어붙인다. `include_dim=false` 면 dim(ghost-suggestion)
/// 셀의 텍스트를 건너뛴다(공백으로 치환하지 않음 — 트리밍 후 자연스럽게 사라지도록).
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

    /// Get the last N lines of terminal output, counted from the **end of the
    /// content** rather than the bottom edge of the grid.
    ///
    /// The screen grid is almost always taller than what the shell has printed, so
    /// the rows below the cursor are blank. Slicing the bottom N grid rows therefore
    /// returns nothing but blank lines whenever the content sits near the top — the
    /// exact situation after a fresh shell prompt or a couple of commands. Instead we
    /// find the last row that still has content, take up to N rows ending there, and
    /// top the result up from scrollback when the screen alone cannot supply N lines.
    /// That makes `n` mean the same thing at every magnitude and matches what the CLI
    /// documents (`--lines`: "from the bottom, dips into scrollback if needed").
    ///
    /// Only the **run of blank rows at the bottom of the screen grid** is skipped.
    /// Blank lines *inside* the content are real output, so they are kept and counted
    /// toward `n`, and lines pulled from scrollback are counted as they come — a blank
    /// line at the tail of the scrollback stays in the result. If the screen and the
    /// scrollback together hold fewer than `n` lines, everything available is returned.
    ///
    /// The scrollback top-up applies on the alternate screen too: an alt screen has no
    /// scrollback of its own, so a TUI that leaves its lower rows blank is filled out
    /// of the primary scrollback. That matches what the previous implementation already
    /// did whenever `n` exceeded the screen height.
    ///
    /// Emptiness is judged with the same `include_dim` the caller asked for, so
    /// `--show-dim` never changes how many lines come back for the same buffer: with
    /// `include_dim=false` a row holding only a dim ghost-suggestion counts as blank,
    /// which is consistent with those cells being excluded from the output anyway.
    pub fn screen_text_lines(&self, n: usize, include_dim: bool) -> String {
        let surface = self.surface();
        let screen_lines = surface.screen_lines();
        let scrollback_total = self.scrollback_len();

        // Render each screen row once and reuse it for both the emptiness test and
        // the output — `line_text` walks every cell, so computing it twice per row
        // would double the work for no gain.
        let rendered: Vec<String> = screen_lines
            .iter()
            .map(|line| line_text(line, include_dim).trim_end().to_string())
            .collect();

        // One past the last row that has content. A `trim_end`-ed row is empty
        // exactly when the raw row was all whitespace, so `is_empty()` here is the
        // same test as `line_text(..).trim().is_empty()`.
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

    /// SGR 2 = faint/dim (`Intensity::Half`) — Claude Code 등 CLI 의
    /// ghost-suggestion 오버레이가 실측으로 확인된 렌더링 신호.
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

    // ── screen_text_lines: content-based "last N lines" ──
    //
    // 이 블록의 시나리오는 전부 `new_detached`(PTY·스레드 없음)로 구성한다 —
    // 플랫폼 의존 없음.

    /// 화면 높이보다 짧은 내용 + 작은 `n`. 예전 구현은 grid 하단 N 행을 그대로 잘라
    /// 공백만 담긴 `"\n"` 을 돌려줬다 — 실사용에서 살아 있는 터미널을 죽은 것으로
    /// 오판하게 만든 결함이다.
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

    /// 하단 공백 스킵 + 스크롤백 채움이 **동시에** 걸리는 경우. 화면 내용이 `n` 보다
    /// 적으면서 그 아래가 공백인 상태 — 결함이 실제로 관측된 형태다.
    #[test]
    fn screen_text_lines_fills_from_scrollback_when_screen_has_trailing_blanks() {
        let mut t = Terminal::new_detached(20, 6);
        // 6 행 화면에 8 줄 → 앞 2 줄이 스크롤백, 화면엔 6 줄.
        t.feed_bytes(b"p0\r\np1\r\np2\r\np3\r\np4\r\np5\r\np6\r\np7");
        assert_eq!(t.scrollback_len(), 2);
        // 화면을 지우고 위쪽 2 줄만 다시 그려 하단 4 행을 공백으로 만든다.
        t.feed_bytes(b"\x1b[2J\x1b[H");
        t.feed_bytes(b"q0\r\nq1");

        // 화면 내용 2 줄 + 스크롤백에서 2 줄. 예전 구현은 하단 3 행(전부 공백)을 잘라
        // 빈 결과를 냈다.
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

    /// 화면이 하단까지 내용으로 차 있는 TUI(alternate screen) 형태에서는
    /// content_end == 화면 높이라 결과가 기존 동작과 동일하다 — 회귀 없음.
    #[test]
    fn screen_text_lines_full_screen_is_plain_bottom_slice() {
        let mut t = Terminal::new_detached(20, 4);
        t.feed_bytes(b"\x1b[?1049h"); // alternate screen 진입
        t.feed_bytes(b"t0\r\nt1\r\nt2\r\nt3");

        assert_eq!(t.screen_text_lines(3, false), "t1\nt2\nt3\n");
    }

    /// **뷰포트 포화는 사양이지 결함이 아니다** — 다만 그 둘이 밖에서 구별되지 않는 것이
    /// 결함이었다. 여기서 두 경우를 나란히 고정한다: 스크롤백이 **없으면** 화면 내용이
    /// 상한이고(줄 수 = `content_end`), **있으면** 그만큼 더 나온다. 상한은 뷰포트가
    /// 아니라 **가진 전부**다.
    ///
    /// 실사용에서 `--lines {20,68,200,400,1000}` 이 `20,68,68,68,68` 로 나온 것이
    /// 이 첫 번째 경우다 — TUI 가 프롬프트 몇 줄 뒤 바로 화면을 점유해 primary 에서
    /// 아무것도 스크롤아웃되지 않은 상태였다. 호출자가 그걸 알 수단이 없어서
    /// "명령이 안 먹는다" 로 읽혔다.
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

        // ② 같은 화면인데 스크롤백이 있으면 그만큼 더 나온다. 뷰포트가 천장이 아니다.
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

    /// alt 스크린에서 `n` 이 화면 높이를 넘으면 **primary 스크롤백**에서 채운다.
    /// `screen_text_lines` 의 독스트링이 그렇게 약속하는데 그 경로를 재는 테스트가
    /// 없었다 — 바로 위 테스트는 `n < 화면 높이` 만 본다. 에이전트가 TUI surface 를
    /// `--lines <큰 수>` 로 읽을 때 실제로 타는 경로가 이쪽이다.
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
