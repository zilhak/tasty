use termwiz::color::ColorAttribute;

pub(crate) const DEFAULT_FG: [f32; 4] = [0.8, 0.8, 0.8, 1.0]; // #cccccc
pub const DEFAULT_BG: [f32; 4] = [0.102, 0.102, 0.118, 1.0]; // #1a1a1e

/// Standard 16-color ANSI palette (sRGB, approximate).
pub(crate) const ANSI_COLORS: [[f32; 3]; 16] = [
    [0.0, 0.0, 0.0],       // 0: black
    [0.8, 0.0, 0.0],       // 1: red
    [0.0, 0.8, 0.0],       // 2: green
    [0.8, 0.8, 0.0],       // 3: yellow
    [0.0, 0.0, 0.8],       // 4: blue
    [0.8, 0.0, 0.8],       // 5: magenta
    [0.0, 0.8, 0.8],       // 6: cyan
    [0.75, 0.75, 0.75],    // 7: white
    [0.5, 0.5, 0.5],       // 8: bright black
    [1.0, 0.0, 0.0],       // 9: bright red
    [0.0, 1.0, 0.0],       // 10: bright green
    [1.0, 1.0, 0.0],       // 11: bright yellow
    [0.0, 0.0, 1.0],       // 12: bright blue
    [1.0, 0.0, 1.0],       // 13: bright magenta
    [0.0, 1.0, 1.0],       // 14: bright cyan
    [1.0, 1.0, 1.0],       // 15: bright white
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
