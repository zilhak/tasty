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
