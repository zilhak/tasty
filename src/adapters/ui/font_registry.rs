//! Per-surface font registration into egui.
//!
//! Some surfaces render with egui rather than the GPU cell renderer and reference
//! a per-kind named font family inside egui's `FontDefinitions`. This module
//! registers **one named family per surface kind that has a font override**
//! (`AppearanceSettings::plugin_font_overrides`), named `font_<kind>`, and re-runs
//! the registration whenever the relevant settings change. The host stays
//! kind-agnostic — it never names a specific kind (e.g. `markdown`); it iterates
//! whatever override kinds are registered (`markdown`, `explorer`, …).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::settings::{AppearanceSettings, EffectiveFont};

/// egui named family for a surface kind's override font: `font_<kind>`.
/// e.g. `markdown` → `font_markdown`.
fn family_name_for_kind(kind: &str) -> String {
    format!("font_{kind}")
}

/// Track the most recently applied per-kind font signatures so we only
/// re-register fonts when something actually changed.
#[derive(Default, Debug, Clone)]
pub struct SurfaceFontState {
    /// kind → signature of the last-applied effective font.
    sigs: BTreeMap<String, String>,
    initialized: bool,
}

fn signature(font: &EffectiveFont) -> String {
    format!("{}|{}", font.font_family, font.custom_font_path)
}

/// Compute the current per-kind effective-font signatures for every registered
/// override kind (generic — no kind name hardcoded).
fn current_signatures(appearance: &AppearanceSettings) -> BTreeMap<String, String> {
    appearance
        .plugin_font_overrides
        .keys()
        .map(|kind| {
            let eff = appearance.effective_font_for_kind(kind);
            (kind.clone(), signature(&eff))
        })
        .collect()
}

/// Refresh the per-kind surface font families in egui if any relevant setting
/// changed since the last call.
pub fn refresh_surface_fonts(
    ctx: &egui::Context,
    appearance: &AppearanceSettings,
    state: &mut SurfaceFontState,
) {
    let new_sigs = current_signatures(appearance);

    if state.initialized && state.sigs == new_sigs {
        return;
    }

    let fonts = build_font_definitions(appearance, None);
    ctx.set_fonts(fonts);
    state.sigs = new_sigs;
    state.initialized = true;
}

/// Build a `FontDefinitions` containing the egui defaults, CJK fallback, and one
/// named family per surface-kind override (`font_<kind>`). Optionally also
/// registers a `preview` named family loaded from the given font.
///
/// Settings UI calls this with a preview font to ensure that when it issues
/// `set_fonts(...)` for live preview, it does not clobber the surface
/// families that `refresh_surface_fonts` previously installed.
pub fn build_font_definitions(
    appearance: &AppearanceSettings,
    preview: Option<(&str, &EffectiveFont)>,
) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    // 번들 D2Coding 을 `Monospace` 맨 앞에 — 부팅이 설치한 그 자리다
    // (`crate::gfx::gpu::GpuState::setup_egui_fonts`). 이 함수가 만드는 것은 그것을
    // **덮어쓰는** `set_fonts` 의 인자라, 여기서 다시 안 넣으면 첫 프레임부터 UI 의
    // mono 가 egui 기본 서체로 돌아간다. 조용한 이유는 글자가 멀쩡히 나오기 때문이고,
    // 값으로 드러나는 자리는 mono 칸으로 재는 곳이다 — 파일 핸들러 헤더 경로 예산이
    // 한 칸 6px 대신 7px 로 잡혀 들어가는 경로를 잘랐다.
    // 두 자리가 같은 mono 를 준다는 것은 `the_two_font_stacks_agree_on_the_mono_cell`
    // 이 잰다.
    fonts.font_data.insert(
        "d2coding".to_owned(),
        Arc::new(egui::FontData::from_static(
            crate::font::D2CODING_REGULAR_TTF,
        )),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "d2coding".to_owned());

    // CJK fallback shared across all families.
    let cjk_data = load_system_cjk_font_data();
    if let Some(bytes) = &cjk_data {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes.clone())),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .push("system_cjk".to_owned());
        }
    }

    // 언어팩 `[font]` 폰트를 CJK 뒤, base 패밀리(Proportional·Monospace)의 맨 뒤 폴백으로
    // 붙인다 — `setup_egui_fonts`(src/gfx/gpu/fonts.rs)와 같은 값·같은 헬퍼를 쓴다.
    if let Some(path) = crate::boot::locale::font_env_path() {
        if let Err(e) = tasty_egui_theme::install_locale_font_fallback(&mut fonts, &path) {
            tracing::warn!(
                "locale font at {} could not be installed: {e}",
                path.display()
            );
        }
    }

    // One named family per override kind — host iterates registered kinds instead
    // of hardcoding a specific one. `data_prefix` uses the kind string to keep the
    // internal `font_data` keys unique across kinds.
    for kind in appearance.plugin_font_overrides.keys() {
        let eff = appearance.effective_font_for_kind(kind);
        let family = family_name_for_kind(kind);
        register_surface_family(&mut fonts, &family, kind, &eff, cjk_data.is_some());
    }

    if let Some((slot, eff)) = preview {
        let prefix = format!("preview_{}", slot);
        register_surface_family(&mut fonts, slot, &prefix, eff, cjk_data.is_some());
    }

    fonts
}

fn register_surface_family(
    fonts: &mut egui::FontDefinitions,
    family_name: &str,
    data_prefix: &str,
    eff: &EffectiveFont,
    has_cjk: bool,
) {
    let mut family_fonts: Vec<String> = Vec::new();

    // Custom font file takes priority.
    if !eff.custom_font_path.is_empty() {
        match std::fs::read(&eff.custom_font_path) {
            Ok(bytes) => {
                let key = format!("{}_custom", data_prefix);
                fonts
                    .font_data
                    .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
                family_fonts.push(key);
            }
            Err(e) => {
                tracing::warn!(
                    "failed to load custom font for {} ({}): {e}",
                    family_name,
                    eff.custom_font_path
                );
            }
        }
    }

    // Named system family (skip for empty / "monospace").
    if !eff.font_family.is_empty() && !eff.font_family.eq_ignore_ascii_case("monospace") {
        let font_config = crate::font::FontConfig::new(14.0, "");
        if let Some(bytes) = font_config.load_family_data(&eff.font_family) {
            let key = format!("{}_named", data_prefix);
            fonts
                .font_data
                .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            family_fonts.push(key);
        }
    }

    // Append egui's default monospace stack as fallback so we always render
    // *something* even if the named family lookup failed.
    if let Some(monospace) = fonts.families.get(&egui::FontFamily::Monospace).cloned() {
        for name in monospace {
            if !family_fonts.contains(&name) {
                family_fonts.push(name);
            }
        }
    }

    // Always include CJK fallback last for hangul/kana/hanzi rendering.
    if has_cjk && !family_fonts.iter().any(|n| n == "system_cjk") {
        family_fonts.push("system_cjk".to_owned());
    }

    fonts
        .families
        .insert(egui::FontFamily::Name(family_name.into()), family_fonts);
}

fn load_system_cjk_font_data() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        let path = "C:/Windows/Fonts/malgun.ttf";
        if let Ok(data) = std::fs::read(path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 헤더 경로 줄이 쓰는 글자 크기 — `Theme` 에서 읽는다(리터럴 금지).
    fn caption_px() -> f32 {
        tasty_themes::mocha_fallback().font_size_caption.value()
    }

    /// caption mono 한 칸이 **깔릴 때** 차지하는 폭. 단일 글리프 폭이 아니다 —
    /// egui 가 advance 를 정수 픽셀로 반올림하므로 둘이 다르다.
    fn mono_advance(fonts: egui::FontDefinitions) -> f32 {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        // 폰트 상태는 첫 pass 에서 채워진다. 빈 pass 의 산출물은 볼 것이 없다.
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::monospace(caption_px());
        let w = |s: &str| {
            ctx.fonts(|f| {
                f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
                    .rect
                    .width()
            })
        };
        w("00") - w("0")
    }

    /// 부팅이 설치하는 스택과 이 모듈이 **덮어쓰는** 스택이 같은 mono 를 준다.
    ///
    /// 두 자리가 따로 만들어지고 뒤쪽이 앞쪽을 `set_fonts` 로 지운다. 그래서 뒤쪽이
    /// 번들 mono 를 빠뜨리면 부팅 직후 한 프레임만 D2Coding 이고 그 뒤로는 egui 기본
    /// 서체가 된다 — 글자는 멀쩡히 나오므로 눈으로는 안 잡힌다. 실제로 그 상태였고,
    /// 파일 핸들러 헤더가 한 칸 7px 로 재서 들어가는 경로를 잘랐다.
    #[test]
    fn the_two_font_stacks_agree_on_the_mono_cell() {
        let boot = egui::Context::default();
        crate::gfx::gpu::GpuState::setup_egui_fonts(&boot);
        // 부팅 스택을 같은 방법으로 재려면 그 `FontDefinitions` 가 필요한데
        // `setup_egui_fonts` 는 ctx 에 직접 얹는다. 그래서 여기서는 ctx 를 직접 잰다.
        let _ = boot.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::monospace(caption_px());
        let w = |s: &str| {
            boot.fonts(|f| {
                f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
                    .rect
                    .width()
            })
        };
        let boot_advance = w("00") - w("0");

        let runtime_advance =
            mono_advance(build_font_definitions(&AppearanceSettings::default(), None));

        assert_eq!(
            runtime_advance, boot_advance,
            "부팅 스택과 런타임 스택의 mono 한 칸이 다르다 — 뒤쪽이 앞쪽을 덮어쓰므로 \
             화면에 남는 것은 런타임 쪽이다"
        );
        // 값도 박는다 — 둘이 함께 기본 서체로 돌아가면 위 단정만으로는 안 죽는다.
        assert_eq!(
            tasty_type_geometry::length::LogicalPx(runtime_advance),
            tasty_ui_widgets::tokens::FH_TARGET_MONO_ADVANCE
        );
    }
}
