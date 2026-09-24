//! 종류별 글꼴 설정을 egui의 font_<kind> 패밀리로 등록하고 설정 변경 시 갱신한다.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::settings::{AppearanceSettings, EffectiveFont};

fn family_name_for_kind(kind: &str) -> String {
    format!("font_{kind}")
}

#[derive(Default, Debug, Clone)]
pub struct SurfaceFontState {
    sigs: BTreeMap<String, String>,
    initialized: bool,
}

fn signature(font: &EffectiveFont) -> String {
    format!("{}|{}", font.font_family, font.custom_font_path)
}

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

/// 기본·CJK·종류별 글꼴과 선택적 preview 글꼴을 함께 만든다.
/// preview의 set_fonts가 기존 종류별 글꼴을 없애지 않도록 전체 구성을 제공한다.
pub fn build_font_definitions(
    appearance: &AppearanceSettings,
    preview: Option<(&str, &EffectiveFont)>,
) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    // set_fonts가 기존 구성을 덮으므로 부팅과 같은 D2Coding 우선순위도 다시 넣는다.
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

    // 부팅과 같은 헬퍼로 CJK 뒤에 언어팩 글꼴을 추가한다.
    if let Some(path) = crate::boot::locale::font_env_path() {
        if let Err(e) = tasty_egui_theme::install_locale_font_fallback(&mut fonts, &path) {
            tracing::warn!(
                "locale font at {} could not be installed: {e}",
                path.display()
            );
        }
    }

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

    // 지정한 글꼴을 찾지 못해도 사용할 기본 monospace 패밀리를 남긴다.
    if let Some(monospace) = fonts.families.get(&egui::FontFamily::Monospace).cloned() {
        for name in monospace {
            if !family_fonts.contains(&name) {
                family_fonts.push(name);
            }
        }
    }

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

    fn caption_px() -> f32 {
        tasty_themes::mocha_fallback().font_size_caption.value()
    }

    /// 배치된 두 글자의 차이로 픽셀 반올림까지 반영한 글자 간격을 잰다.
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

    // 부팅 뒤 set_fonts가 기존 구성을 덮어도 같은 monospace 글꼴을 유지해야 한다.
    #[test]
    fn the_two_font_stacks_agree_on_the_mono_cell() {
        let boot = egui::Context::default();
        crate::gfx::gpu::GpuState::setup_egui_fonts(&boot);
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
        // 두 경로가 함께 다른 글꼴로 바뀌는 경우도 확인한다.
        assert_eq!(
            tasty_type_geometry::length::LogicalPx(runtime_advance),
            tasty_ui_widgets::tokens::FH_TARGET_MONO_ADVANCE
        );
    }
}
