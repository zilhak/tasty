use std::sync::Arc;

use crate::font::{D2CODING_FAMILY, D2CODING_REGULAR_TTF};

use super::GpuState;

impl GpuState {
    /// Register fonts for the egui UI:
    /// - Bundled D2Coding ligature is the primary `Monospace` family entry.
    /// - System CJK font (Malgun / AppleSDGothicNeo / NotoSansCJK) is appended
    ///   as fallback for both `Proportional` and `Monospace` so 한글/한자/かな
    ///   render correctly in UI labels.
    /// - `Proportional` itself keeps egui's default UI fonts as the primary face.
    pub(super) fn setup_egui_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "d2coding".to_owned(),
            Arc::new(egui::FontData::from_static(D2CODING_REGULAR_TTF)),
        );
        let monospace = fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default();
        monospace.insert(0, "d2coding".to_owned());

        if let Some(cjk_bytes) = Self::load_system_cjk_font() {
            fonts.font_data.insert(
                "system_cjk".to_owned(),
                Arc::new(egui::FontData::from_owned(cjk_bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("system_cjk".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("system_cjk".to_owned());
        } else {
            tracing::warn!(
                "no system CJK font found; UI may show □ for non-Latin/Hangul text outside D2Coding ({D2CODING_FAMILY})"
            );
        }

        ctx.set_fonts(fonts);
    }

    fn load_system_cjk_font() -> Option<Vec<u8>> {
        #[cfg(target_os = "windows")]
        {
            // Malgun Gothic (맑은 고딕) — bundled with Windows Vista+
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
}
