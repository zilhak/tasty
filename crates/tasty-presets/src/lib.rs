//! `tasty-presets` — Workspace/Tab/Pane 레이아웃 preset.
//!
//! 레이아웃 + 각 leaf surface 의 (kind, cwd, startup command, kind 별 params) 를
//! 디스크 toml 로 저장하고 재사용한다. `ClosedItem` (인메모리 복원) 과 별개.
//!
//! 이 crate 는 *데이터 schema + 디스크 IO* 만 책임진다. 라이브 Workspace/Tab/Pane
//! 으로부터 캡처하는 로직 (`Surface` trait 호출 + 트리 walking) 은 본 바이너리의
//! `intent::preset_capture` 모듈이 담당. 즉 presets crate 는 `tasty-core::model` /
//! `Surface` trait 에 의존하지 않는다.

pub mod model;
pub mod storage;

pub use model::{
    LayoutPreset, PanePreset, PresetKind, PresetPane, PresetPaneNode, PresetSplitDirection,
    PresetSurface, PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};
pub use storage::{PresetError, PresetResult, PresetStore};

// ── Capture 콜백 표면 (capture 로직은 본 바이너리 측) ────────────────────

/// 한 surface 의 (kind, params) snapshot.
///
/// 본 바이너리의 capture 로직이 SurfaceKindRegistry 의 snapshot 을 호출해 만들어
/// 넘긴다. presets crate 는 이 struct 만 알고, 그 위의 `dyn Surface` 같은
/// trait 은 모른다.
#[derive(Debug, Clone)]
pub struct CapturedSurfaceMeta {
    pub kind: String,
    pub params: serde_json::Value,
}
