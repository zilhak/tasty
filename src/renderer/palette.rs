use termwiz::color::ColorAttribute;

use crate::theme;

pub(crate) fn default_fg() -> [f32; 4] {
    theme::theme().terminal_fg
}

pub fn default_bg() -> [f32; 4] {
    theme::theme().terminal_bg
}

pub(crate) fn ansi_colors() -> &'static [[f32; 3]; 16] {
    &theme::theme().ansi_colors
}

// Keep the old constants as aliases for backward compatibility in other modules
pub(crate) const DEFAULT_FG: [f32; 4] = [0.804, 0.839, 0.957, 1.0]; // Text #cdd6f4
pub const DEFAULT_BG: [f32; 4] = [0.118, 0.118, 0.180, 1.0]; // Base #1e1e2e

/// Catppuccin Mocha 16-color ANSI palette.
pub(crate) const ANSI_COLORS: [[f32; 3]; 16] = [
    [0.176, 0.176, 0.271],  // 0: black      (Surface1 #45475a)
    [0.953, 0.545, 0.659],  // 1: red         (#f38ba8)
    [0.651, 0.890, 0.631],  // 2: green       (#a6e3a1)
    [0.976, 0.886, 0.686],  // 3: yellow      (#f9e2af)
    [0.537, 0.706, 0.980],  // 4: blue        (#89b4fa)
    [0.796, 0.651, 0.969],  // 5: magenta     (#cba6f7)
    [0.580, 0.886, 0.835],  // 6: cyan        (#94e2d5)
    [0.729, 0.761, 0.882],  // 7: white       (Subtext1 #bac2de)
    [0.424, 0.439, 0.537],  // 8: bright black(Overlay0 #6c7086)
    [0.953, 0.545, 0.659],  // 9: bright red  (#f38ba8)
    [0.651, 0.890, 0.631],  // 10: bright green(#a6e3a1)
    [0.976, 0.886, 0.686],  // 11: bright yellow(#f9e2af)
    [0.537, 0.706, 0.980],  // 12: bright blue(#89b4fa)
    [0.796, 0.651, 0.969],  // 13: bright magenta(#cba6f7)
    [0.537, 0.784, 0.922],  // 14: bright cyan(Sky #89dceb)
    [0.804, 0.839, 0.957],  // 15: bright white(Text #cdd6f4)
];

pub(crate) fn palette_index_to_rgb(idx: u8) -> [f32; 3] {
    if idx < 16 {
        ANSI_COLORS[idx as usize]
    } else if idx < 232 {
        // 216-color cube: 6x6x6
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| if v == 0 { 0.0 } else { (55.0 + 40.0 * v as f32) / 255.0 };
        [to_f(r), to_f(g), to_f(b)]
    } else {
        // 24 grayscale: 232..=255
        let level = (8 + 10 * (idx - 232) as u16) as f32 / 255.0;
        [level, level, level]
    }
}

pub(crate) fn color_attr_to_rgba(attr: &ColorAttribute, default: [f32; 4]) -> [f32; 4] {
    match attr {
        ColorAttribute::Default => default,
        ColorAttribute::PaletteIndex(idx) => {
            let [r, g, b] = palette_index_to_rgb(*idx);
            [r, g, b, 1.0]
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _) => {
            [srgba.0, srgba.1, srgba.2, srgba.3]
        }
        ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
            [srgba.0, srgba.1, srgba.2, srgba.3]
        }
    }
}
