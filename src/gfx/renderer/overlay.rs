//! 강조색은 별도 도형이 아니라 셀 색을 바꿔 표시한다. GPU 배경 pass가 REPLACE이므로
//! 반투명 강조를 셀 배경과 먼저 합성해야 아래 clear 색과 섞이지 않는다.

use tasty_type_appearance::color::GpuRgba;

/// `top` 을 `base` 위에 source-over 로 합성한다(straight alpha).
///
/// 불투명한 `top` 은 그대로 돌려준다 — 강조색이 불투명인 테마(빌트인 테마의
/// 선택 · vi 커서 · 링크)는 합성 전과 비트 단위로 같다.
pub(crate) fn composite_over(top: GpuRgba, base: GpuRgba) -> GpuRgba {
    let ta = top.a().clamp(0.0, 1.0);
    if ta >= 1.0 {
        return top;
    }
    let ba = base.a().clamp(0.0, 1.0);
    let a = (ta + ba * (1.0 - ta)).min(1.0);
    if a <= 0.0 {
        return top;
    }
    let mix = |t: f32, b: f32| (t * ta + b * ba * (1.0 - ta)) / a;
    GpuRgba::dangerously_force_from_array([
        mix(top.r(), base.r()),
        mix(top.g(), base.g()),
        mix(top.b(), base.b()),
        a,
    ])
}

#[cfg(test)]
mod tests {
    // 테스트는 합성 산술을 재려고 원시 색상값을 직접 만든다(테마 경로 밖).
    use super::*;

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> GpuRgba {
        GpuRgba::dangerously_force_from_array([r, g, b, a])
    }

    fn to_u8(c: GpuRgba) -> [u8; 4] {
        c.as_array().map(|v| (v * 255.0).round() as u8)
    }

    #[test]
    fn opaque_top_replaces_base_exactly() {
        let top = rgba(0.3, 0.4, 0.5, 1.0);
        assert_eq!(composite_over(top, rgba(1.0, 0.0, 0.0, 1.0)), top);
    }

    #[test]
    fn transparent_top_leaves_base() {
        let base = rgba(0.1, 0.2, 0.3, 1.0);
        assert_eq!(
            to_u8(composite_over(rgba(1.0, 1.0, 1.0, 0.0), base)),
            to_u8(base)
        );
    }

    /// mocha 검색 강조(`#f9e2af` @ 0x4d · 0xb3)를 검은 셀 배경 위에 얹은 값.
    /// active 와 inactive 가 서로 다르고, 둘 다 불투명이다.
    #[test]
    fn mocha_search_highlights_differ_over_black() {
        let black = rgba(0.0, 0.0, 0.0, 1.0);
        let yellow = |a: u8| {
            rgba(
                249.0 / 255.0,
                226.0 / 255.0,
                175.0 / 255.0,
                a as f32 / 255.0,
            )
        };
        let inactive = to_u8(composite_over(yellow(0x4d), black));
        let active = to_u8(composite_over(yellow(0xb3), black));
        assert_eq!(inactive, [0x4b, 0x44, 0x35, 0xff]);
        assert_eq!(active, [0xaf, 0x9f, 0x7b, 0xff]);
    }

    #[test]
    fn opaque_base_stays_opaque() {
        let out = composite_over(rgba(1.0, 1.0, 0.0, 0.3), rgba(0.0, 0.0, 1.0, 1.0));
        assert!((out.a() - 1.0).abs() < 1e-6, "alpha {}", out.a());
    }
}
