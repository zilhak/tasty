//! `Terminal` 의 screen / cell 접근 메서드.

use termwiz::cell::{CellAttributes, Underline};
use termwiz::color::ColorAttribute;

use crate::{CellInfo, Terminal};

impl Terminal {
    /// Get the visible text content of the screen as a string.
    /// Each row is on its own line, trailing spaces are trimmed.
    pub fn screen_text(&self) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        let mut result = String::new();
        for line in lines {
            let mut row_text = String::new();
            for cell in line.visible_cells() {
                row_text.push_str(cell.str());
            }
            result.push_str(row_text.trim_end());
            result.push('\n');
        }
        // Trim trailing empty lines
        while result.ends_with("\n\n") {
            result.pop();
        }
        result
    }

    /// Get the last N lines of terminal output (screen + scrollback from the bottom).
    /// If N is larger than available lines, returns everything available.
    pub fn screen_text_lines(&self, n: usize) -> String {
        let surface = self.surface();
        let screen_lines = surface.screen_lines();
        let screen_count = screen_lines.len();
        let scrollback_total = self.scrollback_len();

        if n <= screen_count {
            // Only need lines from the current screen (bottom N rows)
            let start = screen_count - n;
            let mut result = String::new();
            for line in &screen_lines[start..] {
                let mut row_text = String::new();
                for cell in line.visible_cells() {
                    row_text.push_str(cell.str());
                }
                result.push_str(row_text.trim_end());
                result.push('\n');
            }
            while result.ends_with("\n\n") {
                result.pop();
            }
            result
        } else {
            // Need scrollback lines + full screen
            let scrollback_needed = (n - screen_count).min(scrollback_total);
            let scrollback_start = scrollback_total - scrollback_needed;

            let mut result = String::new();

            // Append scrollback lines (from scrollback_start to end)
            for i in scrollback_start..scrollback_total {
                let line_text = self
                    .scrollback_line_owned(i)
                    .map(|cells| cells.iter().map(|(s, _)| s.as_str()).collect::<String>())
                    .unwrap_or_default();
                result.push_str(line_text.trim_end());
                result.push('\n');
            }

            // Append all screen lines
            for line in screen_lines {
                let mut row_text = String::new();
                for cell in line.visible_cells() {
                    row_text.push_str(cell.str());
                }
                result.push_str(row_text.trim_end());
                result.push('\n');
            }

            while result.ends_with("\n\n") {
                result.pop();
            }
            result
        }
    }

    /// Get the text of a specific row (0-indexed), trimmed.
    pub fn screen_row(&self, row: usize) -> String {
        let surface = self.surface();
        let lines = surface.screen_lines();
        if row >= lines.len() {
            return String::new();
        }
        let mut text = String::new();
        for cell in lines[row].visible_cells() {
            text.push_str(cell.str());
        }
        text.trim_end().to_string()
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
                let width = if cell.str().chars().next().map_or(false, |c| {
                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1
                }) {
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
                let width = if cell.str().chars().next().map_or(false, |c| {
                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) > 1
                }) {
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
