//! 미리보기 캐시 칸은 preset 마다 따로다. 한 프레임에 다른 preset 을 그리는 호출이 먼저
//! 와도 편집 중인 preset 의 캐시(사용자가 고치고 있는 트리)는 남아야 하고, 칸은 둘러본
//! preset 수만큼 쌓이지 않아야 한다. [`draw_preview`] 를 헤드리스 egui 프레임으로 돌려
//! 칸에서 잰다.

use super::*;
use tasty_presets::{PresetPane, PresetSurface, PresetTab, WorkspacePreset};

fn terminal_tab() -> PresetTab {
    PresetTab {
        explicit_name: None,
        layout: PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                id: None,
                kind: "terminal".into(),
                cwd: None,
                startup_command: None,
                params: serde_json::Value::Null,
            },
        },
    }
}

/// 탭 `tabs` 개짜리 pane 하나로 된 workspace preset.
fn ws(name: &str, tabs: usize) -> WorkspacePreset {
    WorkspacePreset {
        name: name.into(),
        subtitle: String::new(),
        description: String::new(),
        layout: PresetPaneNode::Leaf {
            pane: PresetPane {
                tabs: (0..tabs).map(|_| terminal_tab()).collect(),
                active_tab: 0,
            },
        },
    }
}

fn key(name: &str) -> String {
    preset_key(PresetKind::Workspace, name)
}

/// 한 프레임 안에서 `calls` 의 (preset 이름, 편집 여부) 를 차례로 `draw_preview` 한다.
fn frame(ctx: &egui::Context, store: &mut PresetStore, calls: &[(&str, bool)]) {
    let theme = tasty_themes::mocha_fallback();
    let catalog = KindCatalog::default();
    let kb = KeybindingSettings::default();
    let mut toasts = ToastManager::new();
    let mut surface_cfg = None;
    let mut selected = None;
    drop(ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.max_rect();
            for &(name, editing) in calls {
                draw_preview(
                    ui,
                    store,
                    &theme,
                    PresetKind::Workspace,
                    name,
                    rect,
                    editing,
                    &mut selected,
                    &mut surface_cfg,
                    &mut toasts,
                    &catalog,
                    &kb,
                );
            }
        });
    }));
}

fn cached(ctx: &egui::Context, name: &str) -> Option<DemoCache> {
    ctx.data(|d| d.get_temp(demo_cache_id(&key(name))))
}

/// 칸에 든 preset `name` 의 미리보기 탭 수.
fn cached_tabs(ctx: &egui::Context, name: &str) -> usize {
    let cache = cached(ctx, name).expect("미리보기 캐시");
    assert_eq!(cache.key, key(name), "칸에 다른 preset 이 들었다");
    count_ws_tabs(&cache.layout.rebuild_pane_node().expect("workspace layout"))
}

/// 저장소에 dev(탭 1)·ops(탭 2) 를 두고, dev 를 편집 모드로 한 프레임 그린 뒤 그 칸에
/// 사용자 편집의 대역(탭 3, 기준 판은 그대로)을 넣는다.
fn editing_dev() -> (tempfile::TempDir, PresetStore, egui::Context) {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut store = PresetStore::load_from(tmp.path().into());
    store.save_workspace(ws("dev", 1)).expect("seed dev");
    store.save_workspace(ws("ops", 2)).expect("seed ops");
    let ctx = egui::Context::default();
    frame(&ctx, &mut store, &[("dev", true)]);
    let mut cache = cached(&ctx, "dev").expect("cache");
    cache.layout = DemoLayout::from_workspace(&ws("dev", 3), &KindCatalog::default());
    ctx.data_mut(|d| d.insert_temp(demo_cache_id(&key("dev")), cache));
    (tmp, store, ctx)
}

/// 같은 프레임에 다른 preset 을 먼저 그려도(보기든 편집이든, 에이전트 저장이 끼었든)
/// 편집 중인 dev 의 캐시는 남는다.
#[test]
fn another_preset_drawn_first_in_the_frame_keeps_the_edit_cache() {
    for first in [("ops", false), ("ops", true)] {
        for agent_saved in [false, true] {
            let (_tmp, mut store, ctx) = editing_dev();
            if agent_saved {
                store
                    .save_workspace_overwrite(ws("dev", 2))
                    .expect("agent save");
            }
            frame(&ctx, &mut store, &[first, ("dev", true)]);
            assert_eq!(
                cached_tabs(&ctx, "dev"),
                3,
                "{first:?} 를 먼저 그린 프레임(에이전트 저장 {agent_saved})이 dev 의 편집을 버렸다"
            );
        }
    }
}

/// 칸은 직전에 그린 프레임과 이번 프레임의 preset 것만 남는다 — preset 을 하나씩 둘러봐도
/// 쌓이지 않고, 다른 preset 으로 옮겼다 돌아오면 저장소에서 새로 짓는다.
#[test]
fn slots_do_not_pile_up_while_browsing() {
    let (_tmp, mut store, ctx) = editing_dev();
    store.save_workspace(ws("qa", 1)).expect("seed qa");

    frame(&ctx, &mut store, &[("ops", false)]);
    assert!(cached(&ctx, "dev").is_some(), "직전 프레임의 칸은 남는다");
    frame(&ctx, &mut store, &[("qa", false)]);
    assert!(
        cached(&ctx, "dev").is_none(),
        "두 프레임 전에 그린 칸이 남았다"
    );
    assert!(
        ctx.data(|d| d.get_temp::<bool>(drew_editing_id(&key("dev"))))
            .is_none(),
        "두 프레임 전에 그린 preset 의 모드 기록이 남았다"
    );

    frame(&ctx, &mut store, &[("dev", false)]);
    assert_eq!(
        cached_tabs(&ctx, "dev"),
        1,
        "돌아온 preset 은 저장소 판이다"
    );
}
