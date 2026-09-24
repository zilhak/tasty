//! 실행 중인 Workspace·Tab·Pane에서 레이아웃 프리셋을 만든다.
//! 트리 순회와 Surface 접근은 여기서 맡고 tasty-presets는 데이터와 디스크 저장을 담당한다.
//!
//! PTY·스크롤백·attach 세션은 보존하지 않는다. 터미널의 params는 비워 두며,
//! 복원을 미룬 EmptySurface는 원래 터미널 또는 플러그인 정보를 보존한다.

use serde_json::Value;

use crate::core::CoreState;
use crate::core::surface_registry::SurfaceKindRegistry;
use crate::model::{
    Deferred, EmptySurface, Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, Workspace,
};
use tasty_presets::{
    PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurface,
    PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};

/// 탭이 없는 pane이 있으면 None을 반환한다.
pub fn capture_workspace_preset(
    engine: &CoreState,
    ws: &Workspace,
    name: Option<String>,
    registry: &SurfaceKindRegistry,
) -> Option<WorkspacePreset> {
    Some(WorkspacePreset {
        name: name.unwrap_or_default(),
        subtitle: ws.subtitle.clone(),
        description: ws.description.clone(),
        layout: capture_pane_node(engine, ws.pane_layout(), registry)?,
    })
}

pub fn capture_tab_preset(
    engine: &CoreState,
    tab: &Tab,
    name: Option<String>,
    registry: &SurfaceKindRegistry,
) -> Option<TabPreset> {
    Some(TabPreset {
        name: name.unwrap_or_default(),
        tab: capture_tab(engine, tab, registry),
    })
}

pub fn capture_pane_preset(
    engine: &CoreState,
    pane: &Pane,
    name: Option<String>,
    registry: &SurfaceKindRegistry,
) -> Option<PanePreset> {
    Some(PanePreset {
        name: name.unwrap_or_default(),
        pane: capture_pane(engine, pane, registry)?,
    })
}

fn to_preset_split(d: SplitDirection) -> PresetSplitDirection {
    match d {
        SplitDirection::Horizontal => PresetSplitDirection::Horizontal,
        SplitDirection::Vertical => PresetSplitDirection::Vertical,
    }
}

fn capture_pane_node(
    engine: &CoreState,
    node: &PaneNode,
    registry: &SurfaceKindRegistry,
) -> Option<PresetPaneNode> {
    match node {
        PaneNode::Leaf(pane) => Some(PresetPaneNode::Leaf {
            pane: capture_pane(engine, pane, registry)?,
        }),
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => Some(PresetPaneNode::Split {
            direction: to_preset_split(*direction),
            ratio: *ratio,
            first: Box::new(capture_pane_node(engine, first, registry)?),
            second: Box::new(capture_pane_node(engine, second, registry)?),
        }),
    }
}

fn capture_pane(
    engine: &CoreState,
    pane: &Pane,
    registry: &SurfaceKindRegistry,
) -> Option<PresetPane> {
    let mut tabs = Vec::with_capacity(pane.tabs.len());
    for tab in &pane.tabs {
        tabs.push(capture_tab(engine, tab, registry));
    }
    if tabs.is_empty() {
        return None;
    }
    let active_tab = pane.active_tab.min(tabs.len() - 1);
    Some(PresetPane { tabs, active_tab })
}

fn capture_tab(engine: &CoreState, tab: &Tab, registry: &SurfaceKindRegistry) -> PresetTab {
    PresetTab {
        explicit_name: tab.explicit_name.clone(),
        layout: capture_surface_layout(engine, tab.layout(), registry),
    }
}

fn capture_surface_layout(
    engine: &CoreState,
    layout: &SurfaceLayout,
    registry: &SurfaceKindRegistry,
) -> PresetSurfaceLayout {
    match layout {
        SurfaceLayout::Leaf(surface) => PresetSurfaceLayout::Leaf {
            surface: capture_surface(engine, surface.as_ref(), registry),
        },
        SurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => PresetSurfaceLayout::Split {
            direction: to_preset_split(*direction),
            ratio: *ratio,
            first: Box::new(capture_surface_layout(engine, first, registry)),
            second: Box::new(capture_surface_layout(engine, second, registry)),
        },
    }
}

/// 등록되지 않은 kind는 empty로 저장해 분할 구조를 보존한다.
/// 등록된 kind의 snapshot이 없으면 params를 빈 객체로 저장한다.
fn capture_surface(
    engine: &CoreState,
    surface: &dyn Surface,
    registry: &SurfaceKindRegistry,
) -> PresetSurface {
    let kind_str = surface.kind();

    // 복원을 미룬 surface의 kind()는 empty이므로 실제 종류를 먼저 확인한다.
    if let Some(es) = surface.as_any().downcast_ref::<EmptySurface>() {
        // Deferred 변종이 추가되면 이 match도 갱신하도록 enum을 직접 검사한다.
        match &es.deferred {
            Some(Deferred::Plugin(p)) => {
                return PresetSurface {
                    id: None,
                    kind: p.kind.clone(),
                    cwd: None,
                    startup_command: None,
                    params: p.snapshot.clone(),
                };
            }
            Some(Deferred::Terminal(spawn)) => {
                let cwd = spawn
                    .working_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string());
                return PresetSurface {
                    // 프리셋 ID는 저장할 때 부여하며 런타임 surface_id를 복사하지 않는다.
                    id: None,
                    kind: "terminal".into(),
                    cwd,
                    startup_command: None,
                    params: Value::Object(Default::default()),
                };
            }
            None => {}
        }
    }

    if kind_str == "terminal" {
        // 프리셋은 로컬에서 실행하므로 원격 cwd를 로컬 작업 경로로 저장하지 않는다.
        let cwd = surface
            .surface_id()
            .and_then(|id| engine.local_surface_cwd(id))
            .map(|p| p.to_string_lossy().to_string());
        return PresetSurface {
            id: None,
            kind: "terminal".into(),
            cwd,
            startup_command: None,
            params: Value::Object(Default::default()),
        };
    }

    if let Some(def) = registry.get(kind_str) {
        let params = (def.snapshot)(surface).unwrap_or_else(|| Value::Object(Default::default()));
        return PresetSurface {
            id: None,
            kind: kind_str.to_string(),
            cwd: None,
            startup_command: None,
            params,
        };
    }

    PresetSurface {
        id: None,
        kind: "empty".into(),
        cwd: None,
        startup_command: None,
        params: Value::Object(Default::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::surface_registry::register_builtin_kinds;
    use crate::model::DeferredSpawn;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn waker() -> tasty_terminal::Waker {
        Arc::new(|| {}) as tasty_terminal::Waker
    }

    fn engine() -> CoreState {
        CoreState::new(80, 24, waker()).expect("CoreState::new")
    }

    fn registry() -> SurfaceKindRegistry {
        let r = SurfaceKindRegistry::new();
        register_builtin_kinds(&r);
        r
    }

    fn deferred_spawn(working_dir: Option<&str>) -> DeferredSpawn {
        DeferredSpawn {
            shell: None,
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            waker: waker(),
            working_dir: working_dir.map(PathBuf::from),
            restore_command: None,
            scrollback_persist_id: None,
        }
    }

    fn leaf_surface(pane: &PanePreset) -> &PresetSurface {
        let tab = pane.pane.tabs.first().expect("at least one tab");
        match &tab.layout {
            PresetSurfaceLayout::Leaf { surface } => surface,
            PresetSurfaceLayout::Split { .. } => panic!("expected leaf layout"),
        }
    }

    #[test]
    fn deferred_empty_surface_captured_as_terminal_with_cwd() {
        let engine = engine();
        let registry = registry();
        let sid = 7;
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new_deferred(
            sid,
            deferred_spawn(Some("/tmp/x")),
        ));
        let pane = Pane::new_with_surface(1, 1, "t".into(), surface);

        let preset = capture_pane_preset(&engine, &pane, None, &registry).expect("capture");
        let leaf = leaf_surface(&preset);

        assert_eq!(leaf.kind, "terminal");
        assert_eq!(leaf.cwd.as_deref(), Some("/tmp/x"));
    }

    #[test]
    fn deferred_without_working_dir_captured_as_terminal_cwd_none() {
        let engine = engine();
        let registry = registry();
        let sid = 8;
        let surface: Box<dyn Surface> =
            Box::new(EmptySurface::new_deferred(sid, deferred_spawn(None)));
        let pane = Pane::new_with_surface(1, 1, "t".into(), surface);

        let preset = capture_pane_preset(&engine, &pane, None, &registry).expect("capture");
        let leaf = leaf_surface(&preset);

        assert_eq!(leaf.kind, "terminal");
        assert_eq!(leaf.cwd, None);
    }

    #[test]
    fn non_deferred_empty_surface_stays_empty() {
        let engine = engine();
        let registry = registry();
        let sid = 9;
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new(sid));
        let pane = Pane::new_with_surface(1, 1, "t".into(), surface);

        let preset = capture_pane_preset(&engine, &pane, None, &registry).expect("capture");
        let leaf = leaf_surface(&preset);

        assert_eq!(leaf.kind, "empty");
        assert_eq!(leaf.cwd, None);
    }

    // 복원을 미룬 플러그인은 터미널로 바꾸지 않고 kind와 snapshot을 보존한다.
    #[test]
    fn plugin_deferred_captured_as_its_kind_with_snapshot() {
        use crate::model::DeferredPlugin;
        let engine = engine();
        let registry = registry();
        let sid = 10;
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new_deferred_plugin(
            sid,
            DeferredPlugin {
                kind: "myplugin".into(),
                snapshot: serde_json::json!({ "state": 42 }),
            },
        ));
        let pane = Pane::new_with_surface(1, 1, "t".into(), surface);

        let preset = capture_pane_preset(&engine, &pane, None, &registry).expect("capture");
        let leaf = leaf_surface(&preset);

        assert_eq!(
            leaf.kind, "myplugin",
            "plugin kind 보존 (terminal 오변환 금지)"
        );
        assert_eq!(
            leaf.params,
            serde_json::json!({ "state": 42 }),
            "snapshot 보존"
        );
        assert_eq!(leaf.cwd, None);
    }
}
