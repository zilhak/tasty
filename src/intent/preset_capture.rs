//! 라이브 Workspace/Tab/Pane → Preset 캡처 (본 바이너리 측).
//!
//! 이전 `tasty-presets::capture` 가 하던 일을 본 바이너리로 옮긴 것. presets crate
//! 는 *데이터 schema + 디스크 IO* 만 책임지고, 트리 walking + Surface trait 호출
//! 같은 *본 바이너리 도메인* 작업은 여기서 한다. presets 가 `tasty-core` 의존
//! 끊는 게 목적 (역의존 금지).
//!
//! 호출자가 `capture_fn` 콜백을 전달한다. 이 콜백은 SurfaceKindRegistry 의
//! snapshot 을 호출해 (kind, params) 를 만든다.
//!
//! terminal surface 의 `cwd` 와 `startup_command` 는 호출자가 제공하지 않고
//! 본 모듈에서 `terminal.get_cwd()` 로 직접 추출. startup_command 는 capture
//! 시점에는 None — 사용자가 PresetWindow 에서 편집한다.

use crate::core::CoreState;
use crate::model::{Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, Workspace};
use tasty_presets::{
    CapturedSurfaceMeta, PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection,
    PresetSurface, PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};

/// SurfaceKindRegistry 의 snapshot 을 부르는 콜백.
///
/// 비-terminal surface 의 kind/params 추출용. None 반환 시 leaf 전체가 None 으로
/// 전파 → 해당 preset 캡처 실패.
pub type CaptureFn<'a> = &'a mut dyn FnMut(&dyn Surface) -> Option<CapturedSurfaceMeta>;

// ── 공개 API ─────────────────────────────────────────────────────────────

/// 라이브 Workspace 를 WorkspacePreset 으로 캡처.
pub fn capture_workspace_preset(
    engine: &CoreState,
    ws: &Workspace,
    name: Option<String>,
    capture_fn: CaptureFn<'_>,
) -> Option<WorkspacePreset> {
    Some(WorkspacePreset {
        name: name.unwrap_or_default(),
        subtitle: ws.subtitle.clone(),
        description: ws.description.clone(),
        layout: capture_pane_node(engine, ws.pane_layout(), capture_fn)?,
    })
}

/// 라이브 Tab 을 TabPreset 으로 캡처.
pub fn capture_tab_preset(
    engine: &CoreState,
    tab: &Tab,
    name: Option<String>,
    capture_fn: CaptureFn<'_>,
) -> Option<TabPreset> {
    Some(TabPreset {
        name: name.unwrap_or_default(),
        tab: capture_tab(engine, tab, capture_fn)?,
    })
}

/// 라이브 Pane 을 PanePreset 으로 캡처.
pub fn capture_pane_preset(
    engine: &CoreState,
    pane: &Pane,
    name: Option<String>,
    capture_fn: CaptureFn<'_>,
) -> Option<PanePreset> {
    Some(PanePreset {
        name: name.unwrap_or_default(),
        pane: capture_pane(engine, pane, capture_fn)?,
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
    capture_fn: CaptureFn<'_>,
) -> Option<PresetPaneNode> {
    match node {
        PaneNode::Leaf(pane) => Some(PresetPaneNode::Leaf {
            pane: capture_pane(engine, pane, capture_fn)?,
        }),
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => Some(PresetPaneNode::Split {
            direction: to_preset_split(*direction),
            ratio: *ratio,
            first: Box::new(capture_pane_node(engine, first, capture_fn)?),
            second: Box::new(capture_pane_node(engine, second, capture_fn)?),
        }),
    }
}

fn capture_pane(engine: &CoreState, pane: &Pane, capture_fn: CaptureFn<'_>) -> Option<PresetPane> {
    let mut tabs = Vec::with_capacity(pane.tabs.len());
    for tab in &pane.tabs {
        tabs.push(capture_tab(engine, tab, capture_fn)?);
    }
    if tabs.is_empty() {
        return None;
    }
    let active_tab = pane.active_tab.min(tabs.len() - 1);
    Some(PresetPane { tabs, active_tab })
}

fn capture_tab(engine: &CoreState, tab: &Tab, capture_fn: CaptureFn<'_>) -> Option<PresetTab> {
    let layout = capture_surface_layout(engine, tab.layout(), capture_fn)?;
    Some(PresetTab {
        explicit_name: tab.explicit_name.clone(),
        layout,
    })
}

fn capture_surface_layout(
    engine: &CoreState,
    layout: &SurfaceLayout,
    capture_fn: CaptureFn<'_>,
) -> Option<PresetSurfaceLayout> {
    match layout {
        SurfaceLayout::Leaf(surface) => Some(PresetSurfaceLayout::Leaf {
            surface: capture_surface(engine, surface.as_ref(), capture_fn)?,
        }),
        SurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => Some(PresetSurfaceLayout::Split {
            direction: to_preset_split(*direction),
            ratio: *ratio,
            first: Box::new(capture_surface_layout(engine, first, capture_fn)?),
            second: Box::new(capture_surface_layout(engine, second, capture_fn)?),
        }),
    }
}

fn capture_surface(
    engine: &CoreState,
    surface: &dyn Surface,
    capture_fn: CaptureFn<'_>,
) -> Option<PresetSurface> {
    let meta = capture_fn(surface)?;
    // terminal kind 면 cwd 추출 (store 에서). 다른 kind 는 params 만.
    let (cwd, startup_command) = if let Some(ts) = surface
        .as_any()
        .downcast_ref::<crate::model::TerminalSurface>()
    {
        let cwd = engine
            .terminals
            .get(ts.id)
            .and_then(|t| t.get_cwd())
            .map(|p| p.to_string_lossy().to_string());
        (cwd, None)
    } else {
        (None, None)
    };
    Some(PresetSurface {
        kind: meta.kind,
        cwd,
        startup_command,
        params: meta.params,
    })
}
