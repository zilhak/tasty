//! Per-surface font registration into egui.
//!
//! Markdown surfaces render with egui rather than the GPU cell renderer, so they
//! need their own family inside egui's `FontDefinitions`. This module registers
//! one named family — "font_markdown" — and re-runs the registration whenever
//! the user changes the relevant settings.

use std::sync::Arc;

use crate::settings::{AppearanceSettings, EffectiveFont};

const MARKDOWN_FAMILY: &str = "font_markdown";

/// Track the most recently applied surface font signatures so we only
/// re-register fonts when something actually changed.
#[derive(Default, Debug, Clone)]
pub struct SurfaceFontState {
    markdown_sig: String,
    initialized: bool,
}

pub fn markdown_family() -> egui::FontFamily {
    egui::FontFamily::Name(MARKDOWN_FAMILY.into())
}

fn signature(font: &EffectiveFont) -> String {
    format!("{}|{}", font.font_family, font.custom_font_path)
}

/// Refresh the markdown font family in egui if the relevant settings have
/// changed since the last call.
pub fn refresh_surface_fonts(
    ctx: &egui::Context,
    appearance: &AppearanceSettings,
    state: &mut SurfaceFontState,
) {
    let md = appearance.effective_font_for_kind("markdown");
    let new_md_sig = signature(&md);

    if state.initialized && state.markdown_sig == new_md_sig {
        return;
    }

    let fonts = build_font_definitions(appearance, None);
    ctx.set_fonts(fonts);
    state.markdown_sig = new_md_sig;
    state.initialized = true;
}

/// Build a `FontDefinitions` containing the egui defaults, CJK fallback, and
/// the per-surface markdown named family. Optionally also registers a
/// `preview` named family loaded from the given font.
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

    let md = appearance.effective_font_for_kind("markdown");
    register_surface_family(&mut fonts, MARKDOWN_FAMILY, "md", &md, cjk_data.is_some());

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
