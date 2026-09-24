#![forbid(unsafe_code)]

//! workspace·tab·pane 레이아웃 preset의 데이터와 TOML 저장소.
//! surface의 kind·cwd·시작 명령·params를 저장한다. 살아 있는 모델의 캡처와 적용은 호스트가 맡는다.

// 이유: 테스트의 let _ =를 제품 코드의 오류 처리 명부에서 제외한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

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
