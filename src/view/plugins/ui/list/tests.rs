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

/// Uninstall 을 누르면 액션 바 자리가 그 자리에서 확인 바로 바뀐다. 긴 상세에서도 질문·안내·두 버튼이
/// 창 안에 보이고, 평소 바의 스위치 라벨은 사라진다. 다른 plugin 의 확인 상태면 평소 바를 그린다.
#[test]
fn uninstall_confirmation_replaces_the_action_bar_in_place() {
    crate::i18n::init("en");
    let mut long = entry();
    long.description = "A plugin with a long detail. ".repeat(40);
    long.manifest_permissions = (0..30).map(|i| format!("perm:{i}")).collect();
    let id = long.id.clone();
    let name = long.name.clone();
    let snapshot = PluginsSnapshot {
        plugins: vec![long],
        ..Default::default()
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 480.0));
    let draw = |confirm: Option<String>| {
        let ctx = egui::Context::default();
        let mut ui_state = PluginsUiState {
            confirm_uninstall_id: confirm,
            ..Default::default()
        };
        let mut actions = Vec::new();
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
        visible_text_rects(&output.expect("drawn"), screen)
    };
    let has = |texts: &[(String, egui::Rect)], label: &str| texts.iter().any(|(t, _)| t == label);

    let confirming = draw(Some(id));
    let title = crate::i18n::t_fmt("plugins.uninstall_confirm_title", &name);
    for label in [
        title.as_str(),
        t("plugins.uninstall_note"),
        t("button.cancel"),
        t("plugins.uninstall"),
    ] {
        assert!(
            has(&confirming, label),
            "{label:?} is not wholly visible in the confirm bar"
        );
    }
    assert!(
        !has(&confirming, t("plugins.enabled")),
        "the normal bar is still drawn while confirming"
    );

    let other = draw(Some("another.plugin".into()));
    assert!(has(&other, t("plugins.enabled")));
    assert!(!has(&other, &title));
}
