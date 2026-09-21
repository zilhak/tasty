//! Headless 빌드용 `dispatch_domain` cascade no-op stubs.
//!
//! gui 빌드의 `dispatch_domain.rs` 는 View 의 모든 window 에 cascade 를 broadcast.
//! headless 에서는 view 자체가 없으므로 cascade 가 의미 없다 — silent no-op.
//!
//! state mutation 만 필요한 일부 cascade (closed_item_restored 등) 도 모두 no-op
//! — headless 의 IPC 표면이 그 state 를 의존하지 않는다 (popup/toast 등 GUI 객체뿐).
//!
//! 구조 변경 cascade(split / tab / close — 자원 회수 포함)는 여기 없다. 두 빌드가 같은
//! 파일을 컴파일하는 `core::structural_cascade` 가 소유하고, gui 와 갈리는 지점은 그 안의
//! `cfg` 블록이다.

#![cfg(not(feature = "gui"))]
#![allow(dead_code, unused_variables)]

use crate::core::CoreState;
use crate::core::intent::RestoredKind;
use crate::intent::IntentOrigin;
use crate::state::AppState;

/// gui 의 `DispatchSource` 와 동등 — headless 는 사용처가 없지만 type path 보존.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DispatchSource {
    Main(u64),
    Parked(usize),
}

/// gui 의 `WorkspaceCreatedCascade` 와 동등.
pub(crate) struct WorkspaceCreatedCascade {
    pub(crate) workspace_id: u32,
    pub(crate) index: usize,
    pub(crate) surface_id: Option<u32>,
    pub(crate) renamed_name: Option<String>,
    pub(crate) renamed_subtitle: Option<String>,
    pub(crate) renamed_description: Option<String>,
}

pub(crate) fn cascade_workspace_created(
    state: &mut AppState,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    window_id: u64,
    c: WorkspaceCreatedCascade,
) {
}

pub(crate) fn cascade_closed_item_restored(
    state: &mut AppState,
    engine: &mut CoreState,
    kind: RestoredKind,
) {
}

/// 다른 stub 과 달리 no-op 이 아니다 — 이 cascade 는 view 가 아니라 `AppState` 의
/// 인덱스 포인터를 고치는 일이라 headless 에도 그대로 필요하다. 제거 축
/// (`core::structural_cascade::cascade_surface_closed` 의
/// `fix_workspace_pointers_after_removal`)과 같은 이유다.
pub(crate) fn cascade_workspace_moved(state: &mut AppState, from_index: usize, to_index: usize) {
    state.fix_workspace_pointers_after_move(from_index, to_index);
}

pub(crate) fn cascade_workspace_meta_updated(
    state: &mut AppState,
    workspace_id: u32,
    name: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
) {
}
