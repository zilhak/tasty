//! Terminal cell color resolution.
//!
//! ANSI 16색 팔레트와 default fg 는 현재 적용된 `Theme` 에서 받아온다. 호출자가
//! 매 프레임 시작 시 한 번 `theme().ansi_palette()` + `theme().terminal_fg.to_float()`
//! 로 추출해 reference 로 넘겨주면, 셀별 lookup 비용은 ANSI 16 인덱싱뿐이다.

use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;

/// 216-color cube + 24 grayscale 영역은 ANSI 16 와 무관 (xterm 표준). 그래서
/// `ansi` 인자는 16색 슬라이스만 받는다.
pub(crate) fn palette_index_to_rgb(idx: u8, ansi: &[[f32; 3]; 16]) -> [f32; 3] {
    if idx < 16 {
        ansi[idx as usize]
    } else if idx < 232 {
        // 216-color cube: 6x6x6
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| {
            if v == 0 {
                0.0
            } else {
                (55.0 + 40.0 * v as f32) / 255.0
            }
        };
        [to_f(r), to_f(g), to_f(b)]
    } else {
        // 24 grayscale: 232..=255
        let level = (8 + 10 * (idx - 232) as u16) as f32 / 255.0;
        [level, level, level]
    }
}

/// Resolve the (bg, fg) RGBA pair that the renderer sends to the GPU for a cell
/// **based purely on its `CellAttributes`**.
///
/// Includes:
/// - foreground/background lookup against `default_bg` and `default_fg`
/// - SGR 7 (reverse) swap
/// - SGR 2 (`Intensity::Half`) — fg is lerped 50:50 toward bg so the glyph
///   fades into the surrounding background. Applied after the reverse swap.
///
/// Does NOT include:
/// - per-cell context overrides (selection bg, link highlight, cursor swap, IME preedit)
pub fn compute_cell_colors(
    attrs: &CellAttributes,
    default_bg: [f32; 4],
    default_fg: [f32; 4],
    ansi: &[[f32; 3]; 16],
) -> ([f32; 4], [f32; 4]) {
    let mut bg = color_attr_to_rgba(&attrs.background(), default_bg, ansi);
    let mut fg = color_attr_to_rgba(&attrs.foreground(), default_fg, ansi);
    if attrs.reverse() {
        std::mem::swap(&mut bg, &mut fg);
    }
    if attrs.intensity() == termwiz::cell::Intensity::Half {
        fg[0] = (fg[0] + bg[0]) * 0.5;
        fg[1] = (fg[1] + bg[1]) * 0.5;
        fg[2] = (fg[2] + bg[2]) * 0.5;
    }
    (bg, fg)
}

pub(crate) fn color_attr_to_rgba(
    attr: &ColorAttribute,
    default: [f32; 4],
    ansi: &[[f32; 3]; 16],
) -> [f32; 4] {
    match attr {
        ColorAttribute::Default => default,
        ColorAttribute::PaletteIndex(idx) => {
            let [r, g, b] = palette_index_to_rgb(*idx, ansi);
            [r, g, b, 1.0]
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _) => {
            [srgba.0, srgba.1, srgba.2, srgba.3]
        }
        ColorAttribute::TrueColorWithDefaultFallback(srgba) => [srgba.0, srgba.1, srgba.2, srgba.3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::cell::Intensity;
    use termwiz::color::{ColorAttribute, SrgbaTuple};

    /// 테스트용 더미 ANSI 16색 팔레트.
    const TEST_ANSI: [[f32; 3]; 16] = [[0.0; 3]; 16];
    const TEST_DEFAULT_FG: [f32; 4] = [0.8, 0.8, 0.95, 1.0];

    #[test]
    fn normal_intensity_keeps_default_fg() {
        let attrs = CellAttributes::default();
        let bg = [0.0, 0.0, 0.0, 1.0];
        let (out_bg, out_fg) = compute_cell_colors(&attrs, bg, TEST_DEFAULT_FG, &TEST_ANSI);
        assert_eq!(out_bg, bg);
        assert_eq!(out_fg, TEST_DEFAULT_FG);
    }

    #[test]
    fn dim_lerps_fg_halfway_toward_bg() {
        let mut attrs = CellAttributes::default();
        attrs.set_intensity(Intensity::Half);
        let bg = [0.0, 0.0, 0.0, 1.0];
        let (out_bg, out_fg) = compute_cell_colors(&attrs, bg, TEST_DEFAULT_FG, &TEST_ANSI);
        assert_eq!(out_bg, bg);
        // TEST_DEFAULT_FG lerped 50:50 toward [0,0,0]
        assert!((out_fg[0] - TEST_DEFAULT_FG[0] * 0.5).abs() < 1e-6);
        assert!((out_fg[1] - TEST_DEFAULT_FG[1] * 0.5).abs() < 1e-6);
        assert!((out_fg[2] - TEST_DEFAULT_FG[2] * 0.5).abs() < 1e-6);
        assert_eq!(out_fg[3], 1.0);
        assert_ne!(out_fg, TEST_DEFAULT_FG);
    }

    #[test]
    fn dim_applies_after_reverse_swap() {
        let mut attrs = CellAttributes::default();
        attrs.set_intensity(Intensity::Half);
        attrs.set_reverse(true);
        attrs.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
            1.0, 1.0, 1.0, 1.0,
        )));
        attrs.set_background(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
            0.0, 0.0, 0.0, 1.0,
        )));
        let default_bg = [0.5, 0.5, 0.5, 1.0];
        let (bg, fg) = compute_cell_colors(&attrs, default_bg, TEST_DEFAULT_FG, &TEST_ANSI);
        // After reverse: bg=[1,1,1], fg=[0,0,0]. After dim (lerp toward bg): fg=[0.5,0.5,0.5].
        assert_eq!(bg, [1.0, 1.0, 1.0, 1.0]);
        assert!((fg[0] - 0.5).abs() < 1e-6);
        assert!((fg[1] - 0.5).abs() < 1e-6);
        assert!((fg[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn palette_index_picks_from_ansi_for_index_below_16() {
        let mut ansi = [[0.0; 3]; 16];
        ansi[1] = [0.9, 0.2, 0.2]; // ANSI red
        let rgb = palette_index_to_rgb(1, &ansi);
        assert_eq!(rgb, [0.9, 0.2, 0.2]);
    }

    #[test]
    fn palette_index_color_cube_independent_of_ansi() {
        let ansi = [[1.0; 3]; 16]; // ANSI 다 흰색
        // 16 (color cube start) 은 ANSI 영향 받지 않아야.
        let rgb = palette_index_to_rgb(16, &ansi);
        assert_eq!(rgb, [0.0, 0.0, 0.0]);
    }
}
