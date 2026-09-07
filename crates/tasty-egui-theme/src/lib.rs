#![forbid(unsafe_code)]
// 이유: 이 억제는 **테스트 범위 전용**이다. 시험이 임시 파일 정리처럼 결과에 무관한
//       `Result` 를 버리는 자리를 프로덕션 명부에 올리면, 그 명부가 실제 프로덕션 자리를
//       가리키는 뜻을 잃는다 — `tests/let_underscore_documented.rs` 의 명부 순수성 판정이
//       그것을 막는다. 자리마다 붙이지 않고 크레이트 루트 한 줄로 그 범위를 덮는다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

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

/// 보더 1줄 stroke — 굵기는 `Theme` 의 `border_width`(theme.md "보더 항상 1px").
/// 색만 인자로 받는다.
#[inline]
fn stroke1(theme: &Theme, c: HexColor) -> egui::Stroke {
    egui::Stroke::new(theme.border_width.value(), c)
}

/// Apply this theme to an egui context.
///
/// `Theme` 자체가 이미 host UI zoom 배율 (`with_colors_and_zoom`) 을 sizing
/// 토큰에 반영하고 있다고 가정한다 — 여기서 별도 ui_scale 곱셈 없음.
pub fn apply_theme_to_egui(theme: &Theme, ctx: &egui::Context) {
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
    visuals.window_stroke = stroke1(theme, theme.surface0);
    visuals.extreme_bg_color = theme.crust.into();
    visuals.faint_bg_color = theme.surface0.into();
    visuals.code_bg_color = theme.surface0.into();

    // ── Widget 상태별 색상 ──
    // 베이스가 light/dark 라도, fg_stroke 가 다크 기본값으로 남으면 라이트 배경에서
    // 거의 안 보인다. 5가지 상태(noninteractive/inactive/hovered/active/open) 모두
    // bg/weak_bg/bg_stroke/fg_stroke 를 명시한다.
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

    // ── Selection / focus ring ──
    // A2 시범 이식: primitive 직접접근 → semantic 접근자(동일 primitive 리턴, 픽셀 동일).
    // accent-primary 의 ~31% alpha. straight RGBA → to_egui() 가 gamma-aware premultiply.
    const SELECTION_BG_ALPHA: u8 = 80;
    visuals.selection.bg_fill = theme
        .accent_primary()
        .with_alpha(SELECTION_BG_ALPHA)
        .to_egui();
    // focus 외곽선은 디자인 시스템의 2px focus ring (border-focus).
    visuals.selection.stroke =
        egui::Stroke::new(theme.focus_ring_width.value(), theme.border_focus());

    // ── 의미 색상 ── (A2 시범 이식: semantic 접근자 사용)
    visuals.hyperlink_color = theme.accent_primary().into();
    visuals.error_fg_color = theme.accent_danger().into();
    visuals.warn_fg_color = theme.accent_warning().into();

    // ── 텍스트 ──
    // override_text_color 를 박으면 egui 의 weak_text_color() 도 이 색의
    // gamma_multiply 로 파생되므로 라이트/다크 모두 자연스럽게 동작.
    visuals.override_text_color = Some(theme.text.into());

    ctx.set_visuals(visuals);

    // ── Style: 폰트 / spacing ──
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
    // 프로그램적 스크롤(`scroll_to_cursor`/`scroll_to_rect`/`scroll_with_delta`)의
    // 애니메이션을 끈다. egui 기본값은 최대 300ms 로 `docs/design/systems/theme.md`
    // "UI 디자인 규칙" 의 애니메이션 상한(150ms)을 넘고, 스크롤은 입력 직후 피드백이
    // 아니라 콘텐츠 이송이라 같은 표의 "스크롤엔 transition 금지" 쪽에 선다(ADR-0108).
    style.scroll_animation = egui::style::ScrollAnimation::none();
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

/// egui `FontDefinitions` 안에서 언어팩 폰트를 가리키는 키.
const LOCALE_FONT_KEY: &str = "locale_pack";

/// 언어팩이 선언한 폰트 파일을 읽어 붙이지 못한 이유. 어느 쪽이든 호출부는 기본 폰트
/// 스택을 그대로 두고(문자열은 렌더되되 팩 스크립트만 □) 경고를 띄운다.
#[derive(Debug)]
pub enum LocaleFontError {
    /// 파일을 읽지 못했다(경로 없음·권한 등).
    Read(std::io::Error),
    /// 읽었으나 폰트로 파싱되지 않는다.
    Parse,
}

impl std::fmt::Display for LocaleFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocaleFontError::Read(e) => write!(f, "cannot read font file: {e}"),
            LocaleFontError::Parse => write!(f, "file is not a valid font"),
        }
    }
}

impl std::error::Error for LocaleFontError {}

/// 언어팩이 선언한 폰트 파일을 `Proportional`·`Monospace` 양쪽의 **마지막** 폴백으로
/// 붙인다. 라틴 글리프는 기본 폰트를 그대로 쓰고 팩 스크립트만 이 폰트로 흘러 내려간다
/// (혼합 렌더). "전부 팩 폰트" 는 범위 밖이다.
///
/// 붙이기 전에 `ab_glyph`(epaint 가 쓰는 그 파서)로 검증한다 — egui 는 깨진 폰트
/// 데이터에서 복구 경로 없이 panic 하므로, 잘못된 파일은 egui 가 보기 전에 막아야 한다.
/// 실패하면 `fonts` 를 건드리지 않고 `Err` 를 돌려준다 — 호출부는 경고하고 기본 스택을
/// 유지한다. 이 함수는 호스트 두 폰트 경로(`src/gfx/gpu/fonts.rs`·
/// `src/adapters/ui/font_registry.rs`)와 egui UI 를 그리는 plugin 넷
/// (`tasty-plugin-{clipboard-viewer,git-viewer,image,markdown}`)의 **단일 진실
/// 공급원**이다 — 검증이 곧 "어떤 폰트를 거부하는가" 라는 판정이라, 사본을 두면
/// host 는 받고 plugin 은 거부하는 갈림이 조용히 생긴다. plugin 은 그래서 이 크레이트를
/// 직접 의존해 같은 판정기를 쓴다(경로는 `TASTY_LOCALE_FONT` env 로 받는다).
pub fn install_locale_font_fallback(
    fonts: &mut egui::FontDefinitions,
    path: &std::path::Path,
) -> Result<(), LocaleFontError> {
    let bytes = std::fs::read(path).map_err(LocaleFontError::Read)?;
    // 검증만 — 슬라이스로 파싱해 보고 성공하면 바이트는 그대로 egui 로 넘긴다.
    ab_glyph::FontRef::try_from_slice(&bytes).map_err(|_| LocaleFontError::Parse)?;
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
        // 정리 실패는 테스트 결과에 무관하다(임시 디렉토리라 OS 가 회수).
        let _ = std::fs::remove_file(&path);
    }
}
