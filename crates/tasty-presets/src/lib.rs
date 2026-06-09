//! `tasty-presets` — Workspace/Tab/Pane 레이아웃 preset.
//!
//! 레이아웃 + 각 leaf surface 의 (kind, cwd, startup command, kind 별 params) 를
//! 디스크 toml 로 저장하고 재사용한다. `ClosedItem` (인메모리 복원) 과 별개.
//!
//! 이 crate 는 *데이터 schema + 디스크 IO* 만 책임진다. 라이브 Workspace/Tab/Pane
//! 으로부터 캡처하는 로직 (`Surface` trait 호출 + 트리 walking) 은 본 바이너리의
//! `intent::preset_capture` 모듈이 담당. 즉 presets crate 는 `tasty-core::model` /
//! `Surface` trait 에 의존하지 않는다.

mod port;
mod port_impl;

pub mod model;
pub mod storage;
pub mod testing;

pub use model::{
    LayoutPreset, PanePreset, PresetKind, PresetPane, PresetPaneNode, PresetSplitDirection,
    PresetSurface, PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};
pub use port::PresetStorage;
pub use storage::{PresetError, PresetResult, PresetStore};
