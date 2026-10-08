use super::*;
use crate::view::plugins::ui::PluginEntry;
use crate::view::plugins::ui::text_probe::visible_text_rects;

/// 공백 없이 어느 글꼴에서도 720·880 창의 상세 열보다 긴 경로.
const INSTALL_DIR: &str = "/home/tasty/.local/share/tasty/plugins/com.example.image-viewer-with-a-long-plugin-identifier/versions/1.0.0-with-a-long-build-suffix/unpacked-bundle-directory";

fn entry() -> PluginEntry {
    PluginEntry {
        id: "com.example.image-viewer-with-a-long-plugin-identifier".into(),
        name: "Image viewer".into(),
        version: "1.0.0".into(),
        description: String::new(),
        authors: Vec::new(),
        homepage: String::new(),
        enabled: true,
        running: false,
        health_error: false,
        builtin: false,
        surface_kinds: Vec::new(),
        manifest_permissions: Vec::new(),
        commands: Vec::new(),
        log_path: format!("{INSTALL_DIR}/plugin.log"),
        install_dir: INSTALL_DIR.into(),
    }
}

/// 긴 설치 경로에서도 `Open folder` 와 경로 전체가 창 안에 보인다. 버튼을 경로 줄에 두거나
/// 경로를 한 줄로 늘이면 720·880 폭에서 둘 중 하나가 창 밖으로 나간다.
#[test]
fn open_folder_and_long_paths_stay_inside_narrow_windows() {
    // 다른 시험의 전역 번역 초기화와 경쟁하지 않도록 그리기 전에 초기화한다.
    crate::i18n::init("en");
    let snapshot = PluginsSnapshot {
        plugins: vec![entry()],
        ..Default::default()
    };
    let log_line = format!("{}: {INSTALL_DIR}/plugin.log", t("plugins.log_path"));
    for width in [720.0, 880.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1200.0));
        let mut ui_state = PluginsUiState::default();
        let mut actions = Vec::new();
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let output = ctx.run(raw, |ctx| {
            draw_list_tab(ctx, &snapshot, &mut ui_state, &mut actions);
        });
        let texts = visible_text_rects(&output, screen);
        for label in [
            t("plugins.open_folder").to_string(),
            INSTALL_DIR.into(),
            log_line.clone(),
        ] {
            assert!(
                texts.iter().any(|(text, _)| *text == label),
                "width {width}: {label:?} is not wholly visible inside the window"
            );
        }
    }
}
