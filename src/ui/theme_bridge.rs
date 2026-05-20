//! Theme ↔ egui 변환 어댑터.
//!
//! `tasty-core::theme::Theme`은 egui와 독립적인 모델이다.
//! egui Visuals/Style 적용처럼 GUI 라이브러리에 직접 의존하는 헬퍼는
//! 호스트 측인 이 모듈에 모은다.

use egui::emath::GuiRounding as _;
use tasty_core::theme::Theme;

/// `TextEdit::hint_text`에 넘길 placeholder 텍스트를 디자인 시스템의
/// `Theme::placeholder` 색상으로 래핑한다. egui의 기본 `weak_text_color`는
/// `override_text_color`(우리는 `Theme::text`로 설정)에서 파생되므로 다크
/// 테마에서도 본문과 비슷한 밝기로 나오기 쉽다 — 명시적으로 색을 박는다.
pub fn hint_text(text: impl Into<String>) -> egui::RichText {
    let th = tasty_core::theme::theme();
    egui::RichText::new(text).color(egui::Color32::from(th.placeholder))
}

/// Apply this theme to an egui context with UI scale factor.
pub fn apply_theme_to_egui(theme: &Theme, ctx: &egui::Context, ui_scale: f32) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.mantle.into();
    visuals.window_fill = theme.base.into();
    visuals.window_stroke = egui::Stroke::new(1.0, theme.surface0);
    visuals.extreme_bg_color = theme.crust.into();
    visuals.widgets.inactive.bg_fill = theme.base.into();
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, theme.surface0);
    visuals.widgets.hovered.bg_fill = theme.surface0.into();
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, theme.surface1);
    visuals.widgets.active.bg_fill = theme.surface1.into();
    visuals.override_text_color = Some(theme.text.into());
    ctx.set_visuals(visuals);

    // Apply scaled UI text sizes and spacing
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional((theme.font_size_body.value() * ui_scale).round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional((theme.font_size_caption.value() * ui_scale).round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional((theme.font_size_heading.value() * ui_scale * 1.15).round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional((theme.font_size_body.value() * ui_scale).round_ui()),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::monospace((theme.font_size_body.value() * ui_scale).round_ui()),
    );
    style.spacing.item_spacing = egui::vec2(
        (theme.spacing_sm.value() * ui_scale).round_ui(),
        (theme.spacing_xs.value() * ui_scale).round_ui(),
    );
    style.spacing.button_padding = egui::vec2(
        (theme.spacing_sm.value() * ui_scale).round_ui(),
        (theme.spacing_xs.value() * ui_scale).round_ui(),
    );
    ctx.set_style(style);
}
