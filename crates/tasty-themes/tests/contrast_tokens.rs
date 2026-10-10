//! 내장 테마에서 대비 보정 토큰이 디자인이 잰 색·대비를 내는지 확인한다.
//! 디자인 수치는 WCAG 2.x 상대휘도로 sRGB 값에서 잰 것이다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte.toml");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

fn luminance(c: HexColor) -> f64 {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: HexColor, b: HexColor) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn rgb(c: HexColor) -> (u8, u8, u8) {
    (c.r(), c.g(), c.b())
}

#[test]
fn approval_danger_fill_is_the_opaque_mix_the_design_measured() {
    let mocha = tasty_themes::mocha_fallback();
    let latte = latte();
    assert_eq!(rgb(mocha.approval_danger_bg()), (56, 43, 61));
    assert_eq!(rgb(latte.approval_danger_bg()), (236, 214, 222));
    assert_eq!(mocha.approval_danger_bg().a(), 255);
    assert_eq!(latte.approval_danger_bg().a(), 255);
    for (theme, label) in [(&mocha, 9.18), (&latte, 5.80)] {
        let measured = contrast(theme.approval_danger_fg(), theme.approval_danger_bg());
        assert!(
            (measured - label).abs() < 0.01,
            "label {measured:.2} ≠ {label}"
        );
    }
    for (theme, edge) in [(&mocha, 5.43), (&latte, 3.52)] {
        let measured = contrast(theme.approval_danger_border(), theme.surface_raised());
        assert!(
            (measured - edge).abs() < 0.01,
            "edge {measured:.2} ≠ {edge}"
        );
    }
}
