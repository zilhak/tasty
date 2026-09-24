//! Palette supplied by the host for OSC 10/11/12 and OSC 4 color queries.
//! The host updates it when creating the terminal and when changing themes.

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

    /// Encode rgb:RRRR/GGGG/BBBB by repeating each 8-bit channel byte.
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
