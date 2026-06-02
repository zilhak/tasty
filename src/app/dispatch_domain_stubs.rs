//! Headless 빌드용 `dispatch_domain` cascade no-op stubs.
//!
//! gui 빌드의 `dispatch_domain.rs` 는 View 의 모든 window 에 cascade 를 broadcast.
//! headless 에서는 view 자체가 없으므로 cascade 가 의미 없다 — silent no-op.
//!
//! state mutation 만 필요한 일부 cascade (closed_item_restored 등) 도 모두 no-op
//! — headless 의 IPC 표면이 그 state 를 의존하지 않는다 (popup/toast 등 GUI 객체뿐).

#![cfg(not(feature = "gui"))]
#![allow(dead_code, clippy::too_many_arguments, unused_variables)]

use crate::core::intent::{CascadeLevel, RestoredKind};
use crate::core::{Core, CoreState};
use crate::intent::IntentOrigin;
use crate::state::AppState;

/// gui 의 `DispatchSource` 와 동등 — headless 는 사용처가 없지만 type path 보존.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DispatchSource {
    Main(u64),
    Parked(usize),
}

pub(crate) fn cascade_workspace_created(
    state: &mut AppState,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    workspace_id: u32,
    index: usize,
    window_id: u64,
    surface_id: Option<u32>,
    renamed_name: Option<String>,
    renamed_subtitle: Option<String>,
    renamed_description: Option<String>,
) {
}

pub(crate) fn cascade_closed_item_restored(
    state: &mut AppState,
    engine: &mut CoreState,
    kind: RestoredKind,
) {
}

pub(crate) fn cascade_surface_closed(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    cascade_level: CascadeLevel,
    cleanup_targets: Vec<(u32, Option<String>)>,
    closed_tab_ids: Vec<u32>,
    closed_pane_ids: Vec<u32>,
    workspace_id_purged: Option<u32>,
    workspaces_now_empty: bool,
) {
}

pub(crate) fn cascade_surface_split(
    state: &mut AppState,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    workspace_index: usize,
    pane_id: u32,
    new_surface_id: u32,
) {
}

pub(crate) fn cascade_pane_split(
    state: &mut AppState,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    workspace_index: usize,
    original_pane_id: u32,
    new_pane_id: u32,
    new_surface_id: u32,
    direction: crate::model::SplitDirection,
) {
}

pub(crate) fn cascade_workspace_moved(state: &mut AppState, from_index: usize, to_index: usize) {}

pub(crate) fn cascade_workspace_meta_updated(
    state: &mut AppState,
    workspace_id: u32,
    name: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
) {
}
