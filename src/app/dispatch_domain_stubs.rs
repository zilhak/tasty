//! Headless 빌드용 `dispatch_domain` cascade no-op stubs.
//!
//! gui 빌드의 `dispatch_domain.rs` 는 View 의 모든 window 에 cascade 를 broadcast.
//! headless 에서는 view 자체가 없으므로 cascade 가 의미 없다 — silent no-op.
//!
//! state mutation 만 필요한 일부 cascade (closed_item_restored 등) 도 모두 no-op
//! — headless 의 IPC 표면이 그 state 를 의존하지 않는다 (popup/toast 등 GUI 객체뿐).

#![cfg(not(feature = "gui"))]
#![allow(dead_code, unused_variables)]

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

/// gui 의 `SurfaceCloseCascade` 와 동등 — 필드 구성은 dispatch_domain.rs 와 동일하게 유지.
pub(crate) struct SurfaceCloseCascade {
    pub(crate) cascade_level: CascadeLevel,
    pub(crate) cleanup_targets: Vec<(u32, Option<String>)>,
    pub(crate) closed_tab_ids: Vec<u32>,
    pub(crate) closed_pane_ids: Vec<u32>,
    pub(crate) workspace_purged: Option<(usize, u32)>,
    pub(crate) workspaces_now_empty: bool,
    pub(crate) is_user_close: bool,
}

/// gui 의 `PaneSplitCascade` 와 동등.
pub(crate) struct PaneSplitCascade {
    pub(crate) workspace_index: usize,
    pub(crate) original_pane_id: u32,
    pub(crate) new_pane_id: u32,
    pub(crate) new_surface_id: u32,
    pub(crate) direction: crate::model::SplitDirection,
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

pub(crate) fn cascade_surface_closed(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    c: SurfaceCloseCascade,
) {
    // headless: PTY/scrollback/메모리 scope 등 *자원* 만 실제 해제. host event /
    // surface.closed lifecycle 통지는 drain 주체(plugin manager / view)가 없으므로
    // 생략 — 통지를 enqueue 하면 pending 큐가 무한 적재된다.
    // c.closed_tab_ids / c.closed_pane_ids / c.is_user_close: lifecycle 통지용 필드 —
    // drain 주체가 없어 미사용.
    for (sid, pid) in c.cleanup_targets {
        state.cleanup_surface(engine, sid, pid);
    }
    // 활성 포인터 보정은 **gui cascade 와 같은 헬퍼로** 한다. 범위 초과 clamp 만으로는
    // 앞쪽 workspace 가 빠졌을 때 인덱스가 유효한 채 다른 workspace 를 가리킨다.
    //
    // 오늘의 headless 는 `active_workspace` 가 0 을 벗어나지 못해(레이아웃 복원 미적용,
    // `preset.apply` 는 focus 를 강제로 끄고, 워크스페이스 전환은 gui 전용 debug IPC 뿐)
    // 이 분기의 결과가 옛 clamp 와 같다. 그래도 헬퍼를 지나게 두는 이유는, 포인터를
    // 움직이는 headless 경로가 하나라도 생기는 순간 같은 불변식이 빌드 형태에 따라
    // 다르게 성립하기 때문이다 — 그때 여기를 고쳐야 한다는 걸 아무도 기억하지 못한다.
    // 근거 `docs/adr/0113-close-preserves-the-focused-target.md`.
    if let Some((removed_idx, _workspace_id)) = c.workspace_purged {
        state.fix_workspace_pointers_after_removal(removed_idx, engine.workspaces.len());
    }
    if c.workspaces_now_empty {
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
    c: PaneSplitCascade,
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
