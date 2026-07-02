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
    // headless: lifecycle 통지용 인자 — drain 주체가 없어 미사용(_ 접두). 자원 해제만 수행.
    _closed_tab_ids: Vec<u32>,
    _closed_pane_ids: Vec<u32>,
    _workspace_id_purged: Option<u32>,
    workspaces_now_empty: bool,
    _is_user_close: bool,
) {
    // headless: PTY/scrollback/메모리 scope 등 *자원* 만 실제 해제. host event /
    // surface.closed lifecycle 통지는 drain 주체(plugin manager / view)가 없으므로
    // 생략 — 통지를 enqueue 하면 pending 큐가 무한 적재된다.
    for (sid, pid) in cleanup_targets {
        state.cleanup_surface(engine, sid, pid);
    }
    // workspaces 가 비지 않도록 invariant 복구 (gui cascade 와 동일). 빈 engine 은
    // 이후 IPC 명령이 active_workspace 를 index out-of-range 로 만들 수 있다.
    if matches!(cascade_level, CascadeLevel::Workspace)
        && state.active_workspace >= engine.workspaces.len()
        && !engine.workspaces.is_empty()
    {
        state.active_workspace = engine.workspaces.len() - 1;
    }
    if workspaces_now_empty {
        match core.create_default_workspace(engine) {
            Ok(idx) => state.active_workspace = idx,
            Err(e) => tracing::warn!("auto-recreate workspace after SurfaceClosed failed: {e}"),
        }
    }
}

/// gui `cascade_pane_closed_full` 의 headless 등가. pane close 시 닫힌 surface 들의
/// PTY/scrollback 을 실제 해제한다. host event / lifecycle 통지는 생략 (drain 주체 없음).
pub(crate) fn cascade_pane_closed_full(
    state: &mut AppState,
    engine: &mut CoreState,
    pane_id: u32,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    let _ = (pane_id, is_user_close); // headless: lifecycle 통지용 인자 미사용 — 값 drop(Result 아님).
    for (sid, pid) in cleanup_targets {
        state.cleanup_surface(engine, sid, pid);
    }
}

/// gui `cascade_tab_created` 의 headless 등가. host event / baseline 통지는
/// drain 주체(plugin manager / view)가 없으므로 생략 — silent no-op.
pub(crate) fn cascade_tab_created(
    state: &mut AppState,
    engine: &CoreState,
    pane_id: u32,
    tab_id: u32,
    surface_id: u32,
) {
}

/// gui `cascade_tab_closed_full` 의 headless 등가. tab close 시 닫힌 surface 들의
/// PTY/scrollback 을 실제 해제한다. host event / lifecycle 통지는 생략 (drain 주체 없음).
pub(crate) fn cascade_tab_closed_full(
    state: &mut AppState,
    engine: &mut CoreState,
    tab_id: u32,
    pane_id: Option<u32>,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    let _ = (tab_id, pane_id, is_user_close); // headless: lifecycle 통지용 인자 미사용 — 값 drop(Result 아님).
    for (sid, pid) in cleanup_targets {
        state.cleanup_surface(engine, sid, pid);
    }
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
