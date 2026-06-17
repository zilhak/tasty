//! OSC color-query palette.
//!
//! `tasty-terminal` stores no theme — cell colors are `ColorAttribute`s and the
//! actual RGB lives in the renderer. To answer OSC color *queries* (OSC 10/11/12
//! dynamic colors and OSC 4 ANSI palette) with the colors the renderer truly
//! draws, the host plumbs its resolved theme palette in via
//! [`crate::Terminal::set_color_palette`]. The palette is refreshed on terminal
//! creation and on every theme change, so a query always reflects the current
//! theme.

/// 8-bit-per-channel RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl TerminalRgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Encode as the xterm 16-bit-per-channel form `rgb:RRRR/GGGG/BBBB`. Each
    /// 8-bit channel is widened by `* 0x101` (xterm replicates the byte), matching
    /// what xterm and other terminals report so querying apps see consistent
    /// precision.
    pub(crate) fn to_x11_16bit(self) -> String {
        format!(
            "rgb:{:04x}/{:04x}/{:04x}",
            self.r as u16 * 0x101,
            self.g as u16 * 0x101,
            self.b as u16 * 0x101
        )
    }
}

/// Resolved theme palette the terminal answers OSC color queries with.
///
/// `foreground`/`background` are the default text colors; `cursor` is the text
/// cursor color (tasty draws the cursor in the foreground color, so the host
/// passes the fg color here); `ansi` is the 16-entry ANSI palette in the standard
/// order (black, red, green, yellow, blue, magenta, cyan, white, then the eight
/// bright variants) — OSC 4 indices `0..16`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorPalette {
    pub foreground: TerminalRgb,
    pub background: TerminalRgb,
    pub cursor: TerminalRgb,
    pub ansi: [TerminalRgb; 16],
}

impl ColorPalette {
    /// RGB for an OSC 10/11/12-style dynamic color number, or `None` for color
    /// numbers tasty has no source for (mouse/highlight/Tektronix colors,
    /// 13..=19), in which case the query is left unanswered.
    pub(crate) fn dynamic_color(&self, number: u8) -> Option<TerminalRgb> {
        match number {
            10 => Some(self.foreground),
            11 => Some(self.background),
            12 => Some(self.cursor),
            _ => None,
        }
    }

    /// RGB for an OSC 4 ANSI palette index. Only the 16 base ANSI colors are
    /// theme-defined; the 216-color cube and grayscale ramp (indices `16..256`)
    /// are the fixed xterm ramp the renderer computes itself, so queries for those
    /// are not answered here.
    pub(crate) fn ansi_color(&self, index: u8) -> Option<TerminalRgb> {
        self.ansi.get(index as usize).copied()
    }
}
