//! 라이브 Workspace/Tab/Pane → Preset 캡처 (본 바이너리 측).
//!
//! 이전 `tasty-presets::capture` 가 하던 일을 본 바이너리로 옮긴 것. presets crate
//! 는 *데이터 schema + 디스크 IO* 만 책임지고, 트리 walking + Surface trait 호출
//! 같은 *본 바이너리 도메인* 작업은 여기서 한다 (presets 가 `tasty-core` 의존
//! 끊는 게 목적 — 역의존 금지).
//!
//! Preset 은 *layout 템플릿* 이다. PTY 인스턴스 / scrollback / attach 세션 같은
//! 라이브 상태는 보존하지 않는다. `preset_apply::build_leaf_surface` 도 이 의도로
//! 작성되어 있어 `kind == "terminal"` 분기는 `cwd` + `startup_command` 만 사용하고
//! `params` 는 참조하지 않는다. capture 측에서도 terminal 의 params 는 빈 객체로
//! 두면 충분.
//!
//! ### terminal/attached 특수 처리
//!
//! `SurfaceKindRegistry` 의 terminal/attached snapshot 은 의도적으로 `None` 을
//! 반환한다 (PTY 는 host 책임, attached 는 휘발성 marker). preset capture 는 이
//! `None` 을 *실패* 가 아니라 *정상 신호* 로 받아들여야 한다. 같은 코드베이스의
//! `engine::layout_persistence::capture::SavedSurface::capture_surface` 가 동일한
//! 패턴을 쓰며, 본 모듈도 그와 정렬한다.

use serde_json::Value;

use crate::core::CoreState;
use crate::engine::surface_registry::SurfaceKindRegistry;
use crate::model::{
    EmptySurface, Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, Workspace,
};
use tasty_presets::{
    PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurface,
    PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};

// ── 공개 API ─────────────────────────────────────────────────────────────

/// 라이브 Workspace 를 WorkspacePreset 으로 캡처.
///
/// surface 단위 capture 는 항상 성공 (terminal / kind 별 snapshot / empty
/// fallback 중 하나로 귀결). 상위에서 `None` 을 반환하는 유일한 경로는 빈 pane.
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

/// 라이브 Tab 을 TabPreset 으로 캡처.
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

/// 라이브 Pane 을 PanePreset 으로 캡처.
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

// ── 내부 helpers ─────────────────────────────────────────────────────────

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

/// 단일 surface 를 PresetSurface 로 캡처. **절대 실패하지 않는다.**
///
/// 분기 흐름 (`SavedSurface::capture_surface` 모범 답안과 동일 패턴):
///
/// 1. deferred `EmptySurface` (PTY 미복원 터미널 placeholder) → `kind =
///    "terminal"` + `DeferredSpawn.working_dir` 를 cwd 로. `kind()` 는 항상
///    `"empty"` 라 registry 분기에 먹히므로, 그 앞에서 downcast +
///    `is_deferred()` 가드로 가로챈다. (비-deferred empty 는 통과.)
/// 2. terminal / attached → `kind = "terminal"` + `cwd` 추출 + 빈 params.
///    (attached 는 preset 레이아웃 관점에서 terminal 슬롯이다.)
/// 3. 그 외 registry 등록 kind → snapshot 호출. snapshot 이 `None` 이면 빈
///    객체로 fallback (kind 는 그대로 보존).
/// 4. registry 에 없는 kind → `kind = "empty"` 로 치환 (leaf 자체가 사라지면
///    split 구조가 어색해지므로).
fn capture_surface(
    engine: &CoreState,
    surface: &dyn Surface,
    registry: &SurfaceKindRegistry,
) -> PresetSurface {
    let kind_str = surface.kind();

    // deferred EmptySurface 는 `kind()` 가 "empty" 라 아래 registry 분기에 먹혀
    // cwd 를 잃는다. layout 영속화(`SavedSurface::capture_surface`)와 동일하게
    // generic 분기보다 앞에서 downcast + is_deferred 가드로 terminal+cwd 캡처.
    if let Some(es) = surface.as_any().downcast_ref::<EmptySurface>()
        && es.is_deferred()
    {
        let cwd = es
            .deferred_spawn
            .as_ref()
            .and_then(|s| s.working_dir.as_ref())
            .map(|p| p.to_string_lossy().to_string());
        return PresetSurface {
            kind: "terminal".into(),
            cwd,
            startup_command: None,
            params: Value::Object(Default::default()),
        };
    }

    if kind_str == "terminal" || kind_str == "attached" {
        let cwd = surface
            .surface_id()
            .and_then(|id| engine.terminals.get(id))
            .and_then(|t| t.get_cwd())
            .map(|p| p.to_string_lossy().to_string());
        return PresetSurface {
            kind: "terminal".into(),
            cwd,
            startup_command: None,
            params: Value::Object(Default::default()),
        };
    }

    if let Some(def) = registry.get(kind_str) {
        let params = (def.snapshot)(surface).unwrap_or_else(|| Value::Object(Default::default()));
        return PresetSurface {
            kind: kind_str.to_string(),
            cwd: None,
            startup_command: None,
            params,
        };
    }

    PresetSurface {
        kind: "empty".into(),
        cwd: None,
        startup_command: None,
        params: Value::Object(Default::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::surface_registry::register_builtin_kinds;
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
            cols: 80,
            rows: 24,
            waker: waker(),
            working_dir: working_dir.map(PathBuf::from),
            restore_command: None,
            scrollback_persist_id: None,
        }
    }

    /// Pane 안 단일 leaf 의 PresetSurface 를 꺼낸다.
    fn leaf_surface(pane: &PanePreset) -> &PresetSurface {
        let tab = pane.pane.tabs.first().expect("at least one tab");
        match &tab.layout {
            PresetSurfaceLayout::Leaf { surface } => surface,
            PresetSurfaceLayout::Split { .. } => panic!("expected leaf layout"),
        }
    }

    /// deferred EmptySurface (working_dir=Some) → 캡처 시 terminal + cwd 로 박제.
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

    /// working_dir=None deferred → terminal 이되 cwd=None (빈 surface 아님).
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

    /// 회귀 방지: 비-deferred 진짜 empty 는 여전히 kind="empty", cwd=None.
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
}
