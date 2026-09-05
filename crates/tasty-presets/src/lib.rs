#![forbid(unsafe_code)]

//! `tasty-presets` — Workspace/Tab/Pane 레이아웃 preset.
//!
//! 레이아웃 + 각 leaf surface 의 (kind, cwd, startup command, kind 별 params) 를
//! 디스크 toml 로 저장하고 재사용한다. `ClosedItem` (인메모리 복원) 과 별개.
//!
//! 이 crate 는 *데이터 schema + 디스크 IO* 만 책임진다. 라이브 Workspace/Tab/Pane
//! 으로부터 캡처하는 로직 (`Surface` trait 호출 + 트리 walking) 은 본 바이너리의
//! `intent::preset_capture` 모듈이 담당. 즉 presets crate 는 `tasty-core::model` /
//! `Surface` trait 에 의존하지 않는다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
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
