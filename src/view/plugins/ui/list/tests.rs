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

/// 상세가 창보다 길어도 액션 바(스위치 라벨 · Configure · Uninstall)는 본문 스크롤 밖에 남아 창 안에 보인다.
#[test]
fn the_action_bar_stays_inside_the_window_below_a_long_detail() {
    crate::i18n::init("en");
    let mut long = entry();
    long.description = "A plugin with a long detail. ".repeat(40);
    long.manifest_permissions = (0..30).map(|i| format!("perm:{i}")).collect();
    long.commands = (0..20)
        .map(|i| crate::view::plugins::ui::PluginCommandEntry {
            title_key: format!("command {i}"),
            keybinding: Some("Ctrl+Alt+G".into()),
        })
        .collect();
    let snapshot = PluginsSnapshot {
        plugins: vec![long],
        ..Default::default()
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 480.0));
    let mut ui_state = PluginsUiState::default();
    let mut actions = Vec::new();
    let ctx = egui::Context::default();
    let mut output = None;
    for _ in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        output = Some(ctx.run(raw, |ctx| {
            draw_list_tab(ctx, &snapshot, &mut ui_state, &mut actions);
        }));
    }
    let texts = visible_text_rects(&output.expect("drawn"), screen);
    for label in [
        t("plugins.enabled"),
        t("plugins.configure"),
        t("plugins.uninstall"),
    ] {
        assert!(
            texts.iter().any(|(text, _)| text == label),
            "{label:?} is not wholly visible inside the window"
        );
    }
}

/// 확인 단계에 들어간 첫 프레임에 본문 끝의 확인 블록으로 스크롤해, 긴 상세에서도 확인 버튼이 보인다.
#[test]
fn entering_the_uninstall_confirmation_scrolls_the_confirm_buttons_into_view() {
    crate::i18n::init("en");
    let mut long = entry();
    long.description = "A plugin with a long detail. ".repeat(40);
    long.manifest_permissions = (0..30).map(|i| format!("perm:{i}")).collect();
    let id = long.id.clone();
    let snapshot = PluginsSnapshot {
        plugins: vec![long],
        ..Default::default()
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 480.0));
    let ctx = egui::Context::default();
    let mut ui_state = PluginsUiState::default();
    let mut actions = Vec::new();
    // 스크롤 애니메이션이 끝나도록 프레임마다 시간을 넉넉히 진행한다.
    let mut time = 0.0;
    let mut frame = |ui_state: &mut PluginsUiState| {
        time += 1.0;
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(time),
            ..Default::default()
        };
        ctx.run(raw, |ctx| {
            draw_list_tab(ctx, &snapshot, ui_state, &mut actions);
        })
    };
    frame(&mut ui_state);
    // 액션 바의 Uninstall 이 하는 일과 같다.
    ui_state.confirm_uninstall_id = Some(id);
    ctx.data_mut(|d| d.insert_temp(confirm_scroll_id(), true));
    let mut output = frame(&mut ui_state);
    for _ in 0..3 {
        output = frame(&mut ui_state);
    }
    let texts = visible_text_rects(&output, screen);
    assert!(
        texts
            .iter()
            .any(|(text, _)| text == t("plugins.uninstall_confirm")),
        "the confirm button is not visible after entering the confirmation"
    );
}
