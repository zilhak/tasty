//! Theme ↔ egui 변환 어댑터.
//!
//! `tasty_type_appearance::theme::Theme` 은 egui 와 독립적인 schema 다.
//! egui Visuals/Style 적용처럼 GUI 라이브러리에 직접 의존하는 헬퍼는
//! 본체와 갤러리(`tasty-gallery`) 모두에서 공유할 수 있도록 별도 lib
//! crate 로 분리한다.
//!
//! 라이트/다크 베이스는 `theme.is_light` 로 분기하고, 그 위에 모든 위젯 색상
//! (stroke 포함), selection, hyperlink, error/warn, code_bg, faint_bg 를 명시적으로
//! 덮어쓴다. 베이스 기본값에 의존하는 필드를 남기지 않아 라이트 ↔ 다크 전환 시
//! 일부 위젯이 어울리지 않는 톤으로 남는 문제를 막는다.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

/// `TextEdit::hint_text` 에 넘길 placeholder 텍스트를 디자인 시스템의
/// `Theme::placeholder` 색상으로 래핑한다. egui 의 기본 `weak_text_color` 는
/// `override_text_color` (우리는 `Theme::text` 로 설정) 에서 파생되므로 다크
/// 테마에서도 본문과 비슷한 밝기로 나오기 쉽다 — 명시적으로 색을 박는다.
///
/// 본체 binary 시절에는 글로벌 `crate::theme::theme()` 을 직접 호출했지만,
/// lib crate 로 분리하면서 theme 을 인자로 받도록 시그니처를 바꾼다.
pub fn hint_text(theme: &Theme, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(egui::Color32::from(theme.placeholder))
}

#[inline]
fn stroke1(c: HexColor) -> egui::Stroke {
    egui::Stroke::new(1.0, c)
}

/// Apply this theme to an egui context with UI scale factor.
pub fn apply_theme_to_egui(theme: &Theme, ctx: &egui::Context, ui_scale: f32) {
    // ── 베이스: 라이트/다크 분기 ──
    // light()/dark() 의 기본값에 의존하는 필드는 아래에서 거의 모두 덮어쓴다.
    // 그래도 베이스를 맞춰두면 shadow / text_cursor 등 우리가 매핑하지 않는
    // 잔여 필드가 적절한 톤으로 남는다.
    let mut visuals = if theme.is_light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    // ── Panel / Window / Extreme ──
    visuals.panel_fill = theme.mantle.into();
    visuals.window_fill = theme.base.into();
    visuals.window_stroke = stroke1(theme.surface0);
    visuals.extreme_bg_color = theme.crust.into();
    visuals.faint_bg_color = theme.surface0.into();
    visuals.code_bg_color = theme.surface0.into();

    // ── Widget 상태별 색상 ──
    // 베이스가 light/dark 라도, fg_stroke 가 다크 기본값으로 남으면 라이트 배경에서
    // 거의 안 보인다. 5가지 상태(noninteractive/inactive/hovered/active/open) 모두
    // bg/weak_bg/bg_stroke/fg_stroke 를 명시한다.
    visuals.widgets.noninteractive.bg_fill = theme.mantle.into();
    visuals.widgets.noninteractive.weak_bg_fill = theme.mantle.into();
    visuals.widgets.noninteractive.bg_stroke = stroke1(theme.surface0);
    visuals.widgets.noninteractive.fg_stroke = stroke1(theme.text);

    visuals.widgets.inactive.bg_fill = theme.base.into();
    visuals.widgets.inactive.weak_bg_fill = theme.base.into();
    visuals.widgets.inactive.bg_stroke = stroke1(theme.surface0);
    visuals.widgets.inactive.fg_stroke = stroke1(theme.text);

    visuals.widgets.hovered.bg_fill = theme.surface0.into();
    visuals.widgets.hovered.weak_bg_fill = theme.surface0.into();
    visuals.widgets.hovered.bg_stroke = stroke1(theme.surface1);
    visuals.widgets.hovered.fg_stroke = stroke1(theme.text);

    visuals.widgets.active.bg_fill = theme.surface1.into();
    visuals.widgets.active.weak_bg_fill = theme.surface1.into();
    visuals.widgets.active.bg_stroke = stroke1(theme.surface2);
    visuals.widgets.active.fg_stroke = stroke1(theme.text);

    visuals.widgets.open.bg_fill = theme.surface1.into();
    visuals.widgets.open.weak_bg_fill = theme.surface1.into();
    visuals.widgets.open.bg_stroke = stroke1(theme.surface2);
    visuals.widgets.open.fg_stroke = stroke1(theme.text);

    // ── Selection ──
    // blue 의 ~31% alpha. straight RGBA → to_egui() 가 gamma-aware premultiply.
    visuals.selection.bg_fill = theme.blue.with_alpha(80).to_egui();
    visuals.selection.stroke = stroke1(theme.blue);

    // ── 의미 색상 ──
    visuals.hyperlink_color = theme.blue.into();
    visuals.error_fg_color = theme.red.into();
    visuals.warn_fg_color = theme.yellow.into();

    // ── 텍스트 ──
    // override_text_color 를 박으면 egui 의 weak_text_color() 도 이 색의
    // gamma_multiply 로 파생되므로 라이트/다크 모두 자연스럽게 동작.
    visuals.override_text_color = Some(theme.text.into());

    ctx.set_visuals(visuals);

    // ── Style: 폰트 / spacing ──
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

/// 시스템에서 CJK 폰트 파일 (macOS / Linux / Windows) 을 찾아 바이트로 반환한다.
/// 본체 GPU 폰트 셋업과 갤러리 양쪽에서 호출되는 단일 진실 공급원.
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

/// egui Context 에 시스템 CJK 폰트를 `Proportional` / `Monospace` family 양쪽의
/// fallback 으로 등록한다. 시스템 폰트를 못 찾으면 `tracing::warn!` 후 noop.
///
/// 본체 (`src/gfx/gpu/fonts.rs`) 는 번들 D2Coding 우선순위와 결합된 자체
/// `setup_egui_fonts` 를 가지므로 이 함수를 직접 호출하지 않고
/// [`load_system_cjk_font`] 만 재사용한다. 갤러리처럼 추가 폰트가 없는 경우용.
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
