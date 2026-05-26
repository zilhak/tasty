//! Terminal cell color resolution.
//!
//! ANSI 16색 팔레트와 default fg 는 현재 적용된 `Theme` 에서 받아온다. 호출자가
//! 매 프레임 시작 시 한 번 `theme().ansi_palette()` + `theme().terminal_fg.to_gpu_rgba()`
//! 로 추출해 reference 로 넘겨주면, 셀별 lookup 비용은 ANSI 16 인덱싱뿐이다.

use tasty_core::color::{GpuRgb, GpuRgba};
use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;

/// 216-color cube + 24 grayscale 영역은 ANSI 16 와 무관 (xterm 표준). 그래서
/// `ansi` 인자는 16색 슬라이스만 받는다.
pub(crate) fn palette_index_to_rgb(idx: u8, ansi: &[GpuRgb; 16]) -> GpuRgb {
    if idx < 16 {
        ansi[idx as usize]
    } else if idx < 232 {
        // 216-color cube: 6x6x6. xterm 표준 색 큐브 — 외부에서 정의된 픽셀 데이터로 간주.
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
        // 외부 입력 (xterm ANSI 표준 색 큐브 정의) — dangerously_force 정당 사용처.
        GpuRgb::dangerously_force_from_array([to_f(r), to_f(g), to_f(b)])
    } else {
        // 24 grayscale: 232..=255. xterm 표준.
        let level = (8 + 10 * (idx - 232) as u16) as f32 / 255.0;
        // 외부 입력 (xterm ANSI 표준 grayscale 정의) — dangerously_force 정당 사용처.
        GpuRgb::dangerously_force_from_array([level, level, level])
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
    default_bg: GpuRgba,
    default_fg: GpuRgba,
    ansi: &[GpuRgb; 16],
) -> (GpuRgba, GpuRgba) {
    let mut bg = color_attr_to_rgba(&attrs.background(), default_bg, ansi);
    let mut fg = color_attr_to_rgba(&attrs.foreground(), default_fg, ansi);
    if attrs.reverse() {
        std::mem::swap(&mut bg, &mut fg);
    }
    if attrs.intensity() == termwiz::cell::Intensity::Half {
        let bg_a = bg.as_array();
        let fg_a = fg.as_array();
        // dim: fg 를 50% lerp toward bg. lerp 결과는 "theme 색에서 파생된 변형" 으로 간주 —
        // theme 의 두 색에서 만들어진 보간값이라 dangerously_force 정당 사용처.
        fg = GpuRgba::dangerously_force_from_array([
            (fg_a[0] + bg_a[0]) * 0.5,
            (fg_a[1] + bg_a[1]) * 0.5,
            (fg_a[2] + bg_a[2]) * 0.5,
            fg_a[3],
        ]);
    }
    (bg, fg)
}

pub(crate) fn color_attr_to_rgba(
    attr: &ColorAttribute,
    default: GpuRgba,
    ansi: &[GpuRgb; 16],
) -> GpuRgba {
    match attr {
        ColorAttribute::Default => default,
        ColorAttribute::PaletteIndex(idx) => {
            let rgb = palette_index_to_rgb(*idx, ansi).as_array();
            // ANSI palette → RGBA (alpha=1.0). palette 의 모든 GpuRgb 는 theme 또는 xterm 표준
            // 정의에서 온 색이라 alpha 만 1.0 으로 패딩하는 것은 변환 본질에 해당.
            GpuRgba::dangerously_force_from_array([rgb[0], rgb[1], rgb[2], 1.0])
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _) => {
            // 외부 입력 (termwiz ANSI true-color escape) — 사용자 터미널 입력.
            GpuRgba::dangerously_force_from_array([srgba.0, srgba.1, srgba.2, srgba.3])
        }
        ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
            // 외부 입력 (termwiz ANSI true-color escape) — 사용자 터미널 입력.
            GpuRgba::dangerously_force_from_array([srgba.0, srgba.1, srgba.2, srgba.3])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::cell::Intensity;
    use termwiz::color::{ColorAttribute, SrgbaTuple};

    /// 테스트용 더미 ANSI 팔레트. 테스트 외 사용 금지.
    fn test_ansi() -> [GpuRgb; 16] {
        [GpuRgb::dangerously_force_from_array([0.0; 3]); 16]
    }
    fn test_default_fg() -> GpuRgba {
        // 테스트 더미 — dangerously_force 정당.
        GpuRgba::dangerously_force_from_array([0.8, 0.8, 0.95, 1.0])
    }

    #[test]
    fn normal_intensity_keeps_default_fg() {
        let attrs = CellAttributes::default();
        // 테스트 더미.
        let bg = GpuRgba::dangerously_force_from_array([0.0, 0.0, 0.0, 1.0]);
        let fg = test_default_fg();
        let (out_bg, out_fg) = compute_cell_colors(&attrs, bg, fg, &test_ansi());
        assert_eq!(out_bg, bg);
        assert_eq!(out_fg, fg);
    }

    #[test]
    fn dim_lerps_fg_halfway_toward_bg() {
        let mut attrs = CellAttributes::default();
        attrs.set_intensity(Intensity::Half);
        let bg = GpuRgba::dangerously_force_from_array([0.0, 0.0, 0.0, 1.0]);
        let fg = test_default_fg();
        let (out_bg, out_fg) = compute_cell_colors(&attrs, bg, fg, &test_ansi());
        assert_eq!(out_bg, bg);
        let fg_a = fg.as_array();
        let out_fg_a = out_fg.as_array();
        // fg lerped 50:50 toward [0,0,0]
        assert!((out_fg_a[0] - fg_a[0] * 0.5).abs() < 1e-6);
        assert!((out_fg_a[1] - fg_a[1] * 0.5).abs() < 1e-6);
        assert!((out_fg_a[2] - fg_a[2] * 0.5).abs() < 1e-6);
        assert_eq!(out_fg_a[3], 1.0);
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
        // 테스트 더미.
        let default_bg = GpuRgba::dangerously_force_from_array([0.5, 0.5, 0.5, 1.0]);
        let (bg, fg) = compute_cell_colors(&attrs, default_bg, test_default_fg(), &test_ansi());
        let bg_a = bg.as_array();
        let fg_a = fg.as_array();
        // After reverse: bg=[1,1,1], fg=[0,0,0]. After dim (lerp toward bg): fg=[0.5,0.5,0.5].
        assert_eq!(bg_a, [1.0, 1.0, 1.0, 1.0]);
        assert!((fg_a[0] - 0.5).abs() < 1e-6);
        assert!((fg_a[1] - 0.5).abs() < 1e-6);
        assert!((fg_a[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn palette_index_picks_from_ansi_for_index_below_16() {
        let mut ansi = test_ansi();
        // 테스트 더미.
        ansi[1] = GpuRgb::dangerously_force_from_array([0.9, 0.2, 0.2]);
        let rgb = palette_index_to_rgb(1, &ansi);
        assert_eq!(rgb.as_array(), [0.9, 0.2, 0.2]);
    }

    #[test]
    fn palette_index_color_cube_independent_of_ansi() {
        // ANSI 다 흰색이어도 16(color cube 시작) 은 영향 받지 않아야.
        let ansi = [GpuRgb::dangerously_force_from_array([1.0; 3]); 16];
        let rgb = palette_index_to_rgb(16, &ansi);
        assert_eq!(rgb.as_array(), [0.0, 0.0, 0.0]);
    }
}
