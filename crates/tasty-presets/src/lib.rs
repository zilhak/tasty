//! `tasty-presets` — Workspace/Tab/Pane 레이아웃 preset.
//!
//! 레이아웃 + 각 leaf surface 의 (kind, cwd, startup command, kind 별 params) 를
//! 디스크에 toml 로 저장하고 재사용한다. `ClosedItem` (인메모리 복원) 과 별개.

pub mod capture;
pub mod model;
pub mod storage;

pub use capture::{CaptureFn, CaptureOptions, CapturedSurfaceMeta};
pub use model::{
    LayoutPreset, PanePreset, PresetKind, PresetPane, PresetPaneNode, PresetSplitDirection,
    PresetSurface, PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};
pub use storage::{PresetError, PresetResult, PresetStore};
