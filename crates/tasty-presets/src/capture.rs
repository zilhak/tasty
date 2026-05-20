//! 라이브 Workspace/Tab/Pane → Preset 변환.
//!
//! 호출자가 `capture_fn` 콜백을 전달한다. 이 콜백은 SurfaceKindRegistry 의
//! snapshot 을 호출해 (kind, params) 를 만든다. tasty-presets crate 는 registry
//! 자체에 의존하지 않는다 (회피하기 위해 콜백 의존성 주입).
//!
//! terminal surface 의 `cwd` 와 `startup_command` 는 호출자가 제공하지 않고
//! 본 모듈에서 `terminal.get_cwd()` 로 직접 추출. startup_command 는 capture
//! 시점에는 None — 사용자가 PresetWindow 에서 편집한다.

use tasty_core::model::{Pane, PaneNode, SurfaceLayout, Tab, Workspace};

use crate::model::{
    PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurface,
    PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};

/// 한 surface 의 (kind, params) snapshot. registry 콜백이 반환.
#[derive(Debug, Clone)]
pub struct CapturedSurfaceMeta {
    pub kind: String,
    pub params: serde_json::Value,
}

/// SurfaceKindRegistry 의 snapshot 을 부르는 콜백.
///
/// 호출자(본 바이너리) 가 만들어 넘긴다. 비-terminal surface 의 kind/params 추출용.
/// None 반환 시 leaf 전체가 None 으로 전파 → 해당 preset 캡처 실패.
pub type CaptureFn<'a> =
    &'a mut dyn FnMut(&dyn tasty_core::model::Surface) -> Option<CapturedSurfaceMeta>;

#[derive(Debug, Default, Clone)]
pub struct CaptureOptions {
    /// 부여할 preset 이름. None 이면 빈 문자열 — 호출자가 set_name 으로 채워야 함.
    pub name: Option<String>,
}

// ── 공개 API ─────────────────────────────────────────────────────────────

impl WorkspacePreset {
    pub fn from_workspace(
        ws: &Workspace,
        capture_fn: CaptureFn<'_>,
        opts: CaptureOptions,
    ) -> Option<Self> {
        let layout = capture_pane_node(ws.pane_layout(), capture_fn)?;
        Some(WorkspacePreset {
            name: opts.name.unwrap_or_default(),
            subtitle: ws.subtitle.clone(),
            description: ws.description.clone(),
            layout,
        })
    }
}

impl TabPreset {
    pub fn from_tab(tab: &Tab, capture_fn: CaptureFn<'_>, opts: CaptureOptions) -> Option<Self> {
        let preset_tab = capture_tab(tab, capture_fn)?;
        Some(TabPreset {
            name: opts.name.unwrap_or_default(),
            tab: preset_tab,
        })
    }
}

impl PanePreset {
    pub fn from_pane(pane: &Pane, capture_fn: CaptureFn<'_>, opts: CaptureOptions) -> Option<Self> {
        let preset_pane = capture_pane(pane, capture_fn)?;
        Some(PanePreset {
            name: opts.name.unwrap_or_default(),
            pane: preset_pane,
        })
    }
}

// ── 내부 helpers ─────────────────────────────────────────────────────────

fn capture_pane_node(node: &PaneNode, capture_fn: CaptureFn<'_>) -> Option<PresetPaneNode> {
    match node {
        PaneNode::Leaf(pane) => Some(PresetPaneNode::Leaf {
            pane: capture_pane(pane, capture_fn)?,
        }),
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => Some(PresetPaneNode::Split {
            direction: PresetSplitDirection::from(*direction),
            ratio: *ratio,
            first: Box::new(capture_pane_node(first, capture_fn)?),
            second: Box::new(capture_pane_node(second, capture_fn)?),
        }),
    }
}

fn capture_pane(pane: &Pane, capture_fn: CaptureFn<'_>) -> Option<PresetPane> {
    let mut tabs = Vec::with_capacity(pane.tabs.len());
    for tab in &pane.tabs {
        tabs.push(capture_tab(tab, capture_fn)?);
    }
    if tabs.is_empty() {
        return None;
    }
    let active_tab = pane.active_tab.min(tabs.len() - 1);
    Some(PresetPane { tabs, active_tab })
}

fn capture_tab(tab: &Tab, capture_fn: CaptureFn<'_>) -> Option<PresetTab> {
    let layout = capture_surface_layout(tab.layout(), capture_fn)?;
    Some(PresetTab {
        explicit_name: tab.explicit_name.clone(),
        layout,
    })
}

fn capture_surface_layout(
    layout: &SurfaceLayout,
    capture_fn: CaptureFn<'_>,
) -> Option<PresetSurfaceLayout> {
    match layout {
        SurfaceLayout::Leaf(surface) => Some(PresetSurfaceLayout::Leaf {
            surface: capture_surface(surface.as_ref(), capture_fn)?,
        }),
        SurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => Some(PresetSurfaceLayout::Split {
            direction: PresetSplitDirection::from(*direction),
            ratio: *ratio,
            first: Box::new(capture_surface_layout(first, capture_fn)?),
            second: Box::new(capture_surface_layout(second, capture_fn)?),
        }),
    }
}

fn capture_surface(
    surface: &dyn tasty_core::model::Surface,
    capture_fn: CaptureFn<'_>,
) -> Option<PresetSurface> {
    let meta = capture_fn(surface)?;
    // terminal kind 면 cwd 추출. 다른 kind 는 params 만.
    let (cwd, startup_command) = if let Some(ts) = surface.as_terminal_surface() {
        let cwd = ts
            .terminal
            .get_cwd()
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
