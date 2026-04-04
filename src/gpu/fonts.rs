use std::sync::Arc;

use super::GpuState;

impl GpuState {
    /// Load a system CJK font into egui so that Korean/Japanese/Chinese text
    /// renders correctly in the UI (e.g., language selector in Settings).
    pub(super) fn setup_egui_cjk_fonts(ctx: &egui::Context) {
        let font_bytes = Self::load_system_cjk_font();
        let Some(bytes) = font_bytes else {
            tracing::warn!("no system CJK font found; UI may show □ for CJK text");
            return;
        };

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );

        // Append as fallback so Latin text still uses egui's default fonts
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
