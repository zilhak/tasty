#![forbid(unsafe_code)]
// 테스트의 임시 파일 정리 실패는 무시한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

//! 본체와 갤러리에서 공유하는 Theme ↔ egui 어댑터.
//! 테마의 밝기에 맞는 egui 기본값을 고른 뒤 위젯 색, 글꼴, 간격, 그림자를 적용한다.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

/// egui 기본 약한 텍스트 색 대신 Theme의 placeholder 색을 적용한다.
pub fn hint_text(theme: &Theme, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(egui::Color32::from(theme.placeholder))
}

#[inline]
fn stroke1(theme: &Theme, c: HexColor) -> egui::Stroke {
    egui::Stroke::new(theme.border_width.value(), c)
}

/// Theme를 egui에 적용한다. Theme에 이미 UI 배율이 반영되어 있어야 한다.
pub fn apply_theme_to_egui(theme: &Theme, ctx: &egui::Context) {
    // 직접 지정하지 않는 필드도 테마 밝기에 맞도록 기본값을 선택한다.
    let mut visuals = if theme.is_light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    visuals.panel_fill = theme.mantle.into();
    visuals.window_fill = theme.base.into();
    visuals.window_stroke = stroke1(theme, theme.surface0);

    // 위젯에 붙는 팝업은 popover, 독립 창은 modal 그림자를 사용한다.
    // 프레임을 직접 지정하는 호출부도 같은 두 토큰 중에서 선택한다.
    visuals.popup_shadow = theme.shadow_popover().to_egui();
    visuals.window_shadow = theme.shadow_modal().to_egui();
    visuals.extreme_bg_color = theme.crust.into();
    visuals.faint_bg_color = theme.surface0.into();
    visuals.code_bg_color = theme.surface0.into();

    visuals.widgets.noninteractive.bg_fill = theme.mantle.into();
    visuals.widgets.noninteractive.weak_bg_fill = theme.mantle.into();
    visuals.widgets.noninteractive.bg_stroke = stroke1(theme, theme.surface0);
    visuals.widgets.noninteractive.fg_stroke = stroke1(theme, theme.text);

    visuals.widgets.inactive.bg_fill = theme.base.into();
    visuals.widgets.inactive.weak_bg_fill = theme.base.into();
    visuals.widgets.inactive.bg_stroke = stroke1(theme, theme.surface0);
    visuals.widgets.inactive.fg_stroke = stroke1(theme, theme.text);

    visuals.widgets.hovered.bg_fill = theme.surface0.into();
    visuals.widgets.hovered.weak_bg_fill = theme.surface0.into();
    visuals.widgets.hovered.bg_stroke = stroke1(theme, theme.surface1);
    visuals.widgets.hovered.fg_stroke = stroke1(theme, theme.text);

    visuals.widgets.active.bg_fill = theme.surface1.into();
    visuals.widgets.active.weak_bg_fill = theme.surface1.into();
    visuals.widgets.active.bg_stroke = stroke1(theme, theme.surface2);
    visuals.widgets.active.fg_stroke = stroke1(theme, theme.text);

    visuals.widgets.open.bg_fill = theme.surface1.into();
    visuals.widgets.open.weak_bg_fill = theme.surface1.into();
    visuals.widgets.open.bg_stroke = stroke1(theme, theme.surface2);
    visuals.widgets.open.fg_stroke = stroke1(theme, theme.text);

    // Straight RGBA는 to_egui()에서 gamma-aware premultiply로 변환된다.
    const SELECTION_BG_ALPHA: u8 = 80;
    visuals.selection.bg_fill = theme
        .accent_primary()
        .with_alpha(SELECTION_BG_ALPHA)
        .to_egui();
    visuals.selection.stroke =
        egui::Stroke::new(theme.focus_ring_width.value(), theme.border_focus());

    visuals.hyperlink_color = theme.accent_primary().into();
    visuals.error_fg_color = theme.accent_danger().into();
    visuals.warn_fg_color = theme.accent_warning().into();

    visuals.override_text_color = Some(theme.text.into());

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(theme.font_size_body.value().round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(theme.font_size_caption.value().round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional((theme.font_size_heading.value() * 1.15).round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(theme.font_size_body.value().round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::monospace(theme.font_size_body.value().round_ui()),
    );
    style.spacing.item_spacing = egui::vec2(
        theme.spacing_sm.value().round_ui(),
        theme.spacing_xs.value().round_ui(),
    );
    style.spacing.button_padding = egui::vec2(
        theme.spacing_sm.value().round_ui(),
        theme.spacing_xs.value().round_ui(),
    );
    // 스크롤에는 transition을 적용하지 않는다.
    style.scroll_animation = egui::style::ScrollAnimation::none();
    ctx.set_style(style);
}

/// macOS·Linux·Windows의 알려진 시스템 경로에서 CJK 폰트 바이트를 읽는다.
pub fn load_system_cjk_font() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        // Malgun Gothic (맑은 고딕) — bundled with Windows Vista+
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
            return Some(data);
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    None
}

/// 시스템 CJK 폰트를 Proportional·Monospace의 마지막 폴백으로 추가한다.
/// 찾지 못하면 경고만 남긴다. 번들 D2Coding은 설치하지 않으므로 본체와 같은
/// 고정폭 측정에는 사용할 수 없다. 본체와 갤러리는 D2Coding을 먼저 설치한 뒤
/// `load_system_cjk_font`를 사용한다.
pub fn install_cjk_fallback(ctx: &egui::Context) {
    let Some(bytes) = load_system_cjk_font() else {
        tracing::warn!("no system CJK font found; Korean/Japanese/Chinese labels will render as □");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "system_cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(fam)
            .or_default()
            .push("system_cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// egui `FontDefinitions` 안에서 언어팩 폰트를 가리키는 키.
const LOCALE_FONT_KEY: &str = "locale_pack";

/// 언어팩 폰트를 읽거나 검증하지 못한 이유.
pub use tasty_i18n::font::LocaleFontError;

/// 언어팩 폰트를 Proportional·Monospace의 마지막 폴백으로 추가한다.
/// 앞선 폰트에 없는 글리프에만 사용한다. 추가 전에 ab_glyph로 검증하며 실패하면
/// `fonts`를 그대로 두고 오류를 반환한다. 본체와 플러그인이 같은 검증을 사용한다.
pub fn install_locale_font_fallback(
    fonts: &mut egui::FontDefinitions,
    path: &std::path::Path,
) -> Result<(), LocaleFontError> {
    let bytes = tasty_i18n::font::read_validated(path)?;
    fonts.font_data.insert(
        LOCALE_FONT_KEY.to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(fam)
            .or_default()
            .push(LOCALE_FONT_KEY.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod locale_font_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn validated_bytes_are_appended_without_reordering_existing_fonts() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let valid_dir = std::env::temp_dir().join(format!(
            "tasty-locale-font-valid-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&valid_dir).unwrap();
        let path = valid_dir.join("valid.ttf");
        let bytes = include_bytes!("../../tasty-font/assets/D2Coding-ligature-Regular.ttf");
        std::fs::write(&path, bytes).unwrap();
        let mut fonts = egui::FontDefinitions::default();
        let before = fonts.families.clone();
        install_locale_font_fallback(&mut fonts, &path).unwrap();
        assert_eq!(fonts.font_data[LOCALE_FONT_KEY].font.as_ref(), bytes);
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let mut expected = before[&family].clone();
            expected.push(LOCALE_FONT_KEY.to_owned());
            assert_eq!(fonts.families[&family], expected);
        }
        std::fs::remove_dir_all(valid_dir).unwrap();
    }

    #[test]
    fn missing_file_is_a_read_error_and_leaves_fonts_untouched() {
        let mut fonts = egui::FontDefinitions::default();
        let before = fonts.font_data.len();
        let err = install_locale_font_fallback(&mut fonts, std::path::Path::new("/no/such.ttf"));
        assert!(matches!(err, Err(LocaleFontError::Read(_))));
        assert_eq!(fonts.font_data.len(), before);
        assert!(!fonts.font_data.contains_key(LOCALE_FONT_KEY));
    }

    #[test]
    fn non_font_bytes_are_a_parse_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tasty-locale-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("garbage.ttf");
        std::fs::write(&path, b"not a font at all").unwrap();
        let mut fonts = egui::FontDefinitions::default();
        let err = install_locale_font_fallback(&mut fonts, &path);
        assert!(matches!(err, Err(LocaleFontError::Parse)));
        assert!(!fonts.font_data.contains_key(LOCALE_FONT_KEY));
        let _ = std::fs::remove_file(&path);
    }
}
