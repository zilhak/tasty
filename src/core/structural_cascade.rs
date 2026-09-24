//! 구조 변경 뒤 자원 정리·활성 위치 보정·빈 workspace 재생성·알림 등록을 처리한다.
//! GUI dispatcher, IPC, 원격 forward가 공유한다. 알림 큐와 화면 계측은 GUI에서만 사용한다.

use crate::core::cascade_window::CascadeWindow;
use crate::core::intent::CascadeLevel;
use crate::core::origin::IntentOrigin;
use crate::core::{Core, CoreState};

pub(crate) struct SurfaceCloseCascade {
    pub(crate) cascade_level: CascadeLevel,
    pub(crate) cleanup_targets: Vec<(u32, Option<String>)>,
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "the tab.closed host event it feeds has a consumer only in the gui build"
        )
    )]
    pub(crate) closed_tab_ids: Vec<u32>,
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "the pane.closed host event it feeds has a consumer only in the gui build"
        )
    )]
    pub(crate) closed_pane_ids: Vec<u32>,
    pub(crate) workspace_purged: Option<(usize, u32)>,
    pub(crate) workspaces_now_empty: bool,
    pub(crate) is_user_close: bool,
}

impl SurfaceCloseCascade {
    /// 성공한 SurfaceClosed의 후속 처리 입력. 실제 정리 대상은 cleanup_targets로 받는다.
    pub(crate) fn from_surface_closed(
        event: crate::core::intent::CoreEvent,
        is_user_close: bool,
    ) -> Option<Self> {
        let crate::core::intent::CoreEvent::SurfaceClosed {
            surface_id: _,
            closed,
            cascade_level,
            cleanup_targets,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
        } = event
        else {
            return None;
        };
        if !closed {
            return None;
        }
        Some(Self {
            cascade_level,
            cleanup_targets,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
            is_user_close,
        })
    }

    /// 이동하는 A는 살려 두고 교체된 B만 정리한다. 닫힌 구조 정보는 A의 이전 위치를 나타낸다.
    /// 다른 이벤트나 moved=false이면 None이다.
    pub(crate) fn from_move_surface_applied(
        event: crate::core::intent::CoreEvent,
        is_user_close: bool,
    ) -> Option<Self> {
        let crate::core::intent::CoreEvent::MoveSurfaceApplied {
            moved,
            b_cleanup,
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
        } = event
        else {
            return None;
        };
        if !moved {
            return None;
        }
        Some(Self {
            cascade_level,
            cleanup_targets: b_cleanup.into_iter().collect(),
            closed_tab_ids,
            closed_pane_ids,
            workspace_purged,
            workspaces_now_empty,
            is_user_close,
        })
    }
}

pub(crate) struct PaneSplitCascade {
    pub(crate) workspace_index: usize,
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui-only split notices and tutorial observation read it"
        )
    )]
    pub(crate) original_pane_id: u32,
    pub(crate) new_pane_id: u32,
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui-only surface.created notice reads it"
        )
    )]
    pub(crate) new_surface_id: u32,
    #[cfg_attr(
        not(feature = "gui"),
        expect(dead_code, reason = "only the gui-only pane.split notice reads it")
    )]
    pub(crate) direction: crate::model::SplitDirection,
}

/// surface 자원을 정리하고 구조별 알림을 등록한다. 삭제 위치에 맞춰 활성 인덱스를 보정해야
/// 앞쪽 workspace가 빠져도 다른 workspace로 선택이 옮겨가지 않는다.
// 이유: workspace ID는 GUI의 제거 알림에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "the purged workspace id only feeds the gui-only workspace.closed notice"
    )
)]
pub(crate) fn cascade_surface_closed(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    c: SurfaceCloseCascade,
) {
    #[cfg(feature = "gui")]
    let surfaces = c.cleanup_targets.len();
    reclaim_closed_surfaces(
        state,
        engine,
        c.cleanup_targets,
        c.is_user_close,
        Some("cascade"),
    );

    #[cfg(feature = "gui")]
    {
        enqueue_closed_tab_events(state, &c.closed_tab_ids, &c.closed_pane_ids);
        enqueue_closed_pane_events(state, &c.closed_pane_ids);
    }

    debug_assert_eq!(
        matches!(c.cascade_level, CascadeLevel::Workspace),
        c.workspace_purged.is_some(),
        "workspace level cascade 와 제거 위치는 함께 실려야 한다"
    );
    if let Some((removed_idx, workspace_id)) = c.workspace_purged {
        #[cfg(feature = "gui")]
        state.after_workspace_removed(workspace_id, "cascade");
        state.fix_workspace_pointers_after_removal(removed_idx, engine.workspaces.len());
    }

    recreate_workspace_if_now_empty(core, state, engine, c.workspaces_now_empty);

    #[cfg(feature = "gui")]
    if let Some((t0, snapshot)) = crate::close_trace::take_cascade() {
        crate::close_trace::log_total(t0, surfaces, snapshot, "cascade");
    }
}

/// 닫힌 surface 정리를 공유한다. GUI에서만 lifecycle 알림과 선택적 계측 로그를 남긴다.
/// kind는 정리 전에 찾지만 Core가 이미 트리를 바꾼 경우 None일 수 있다.
// 이유: is_user_close와 trace는 GUI 알림·계측에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "the lifecycle notice and the C5 trace it gates have a consumer only in the gui build"
    )
)]
fn reclaim_closed_surfaces(
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
    trace: Option<&'static str>,
) {
    #[cfg(feature = "gui")]
    let t_loop = std::time::Instant::now();
    let mut sums = crate::close_trace::CleanupSums::default();
    for (sid, pid) in cleanup_targets {
        #[cfg(feature = "gui")]
        let kind = engine.find_surface_by_id(sid).map(|s| s.kind());
        state.cleanup_surface_traced(engine, sid, pid, &mut sums);
        #[cfg(feature = "gui")]
        state.enqueue_surface_closed(sid, kind, is_user_close);
    }
    #[cfg(feature = "gui")]
    if let Some(path) = trace {
        sums.log(t_loop.elapsed(), path);
    }
}

/// 닫힌 pane ID가 없으면 lifecycle 캐시에서 탭의 이전 pane을 찾는다. 둘 다 없으면 0을 보낸다.
#[cfg(feature = "gui")]
fn enqueue_closed_tab_events(
    state: &mut dyn CascadeWindow,
    closed_tab_ids: &[u32],
    closed_pane_ids: &[u32],
) {
    for tab_id in closed_tab_ids {
        let pane_id = closed_pane_ids
            .first()
            .copied()
            .unwrap_or_else(|| state.lifecycle_baseline_pane_of(*tab_id).unwrap_or(0));
        state.enqueue_host_event(crate::core::host_event::PendingHostEvent::TabClosed {
            tab_id: *tab_id,
            pane_id,
        });
        state.lifecycle_baseline_remove_tab(*tab_id);
    }
}

#[cfg(feature = "gui")]
fn enqueue_closed_pane_events(state: &mut dyn CascadeWindow, closed_pane_ids: &[u32]) {
    for pane_id in closed_pane_ids {
        state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneClosed {
            pane_id: *pane_id,
        });
    }
}

/// 모든 workspace가 사라졌으면 기본 workspace 생성을 시도한다. 실패는 로그로 남긴다.
fn recreate_workspace_if_now_empty(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    workspaces_now_empty: bool,
) {
    if !workspaces_now_empty {
        return;
    }
    match core.create_default_workspace(engine) {
        Ok(idx) => state.set_active_workspace(idx),
        Err(e) => tracing::warn!("auto-recreate workspace after SurfaceClosed failed: {e}"),
    }
}

/// 생성 알림을 등록하고 User origin일 때만 새 surface를 선택한다.
// 이유: state는 GUI 알림·튜토리얼에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "only the gui-only surface.created notice and tutorial observation touch the app state"
    )
)]
pub(crate) fn cascade_surface_split(
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    workspace_index: usize,
    pane_id: u32,
    new_surface_id: u32,
) {
    #[cfg(feature = "gui")]
    cascade_surface_created(state, engine, new_surface_id);
    if !origin.is_user() {
        return;
    }
    if let Some(ws) = engine.workspaces.get_mut(workspace_index)
        && let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
        && let Some(tab) = pane.active_tab_mut()
    {
        tab.focused_surface = new_surface_id;
    }
    #[cfg(feature = "gui")]
    state.observe_tutorial_surface_split(engine, workspace_index, pane_id, new_surface_id);
}

/// 분할 알림과 lifecycle 상태를 갱신한다. User origin일 때만 새 pane을 선택한다.
// 이유: state는 GUI 알림·튜토리얼에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "only the gui-only split notices and tutorial observation touch the app state"
    )
)]
pub(crate) fn cascade_pane_split(
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    origin: &IntentOrigin,
    c: PaneSplitCascade,
) {
    #[cfg(feature = "gui")]
    let workspace_id = {
        state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneSplit {
            original_pane: c.original_pane_id,
            new_pane: c.new_pane_id,
            direction: c.direction,
        });
        let workspace_id = engine.workspaces.get(c.workspace_index).map(|w| w.id);
        if let Some(workspace_id) = workspace_id {
            state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneCreated {
                pane_id: c.new_pane_id,
                workspace_id,
            });
        }
        cascade_surface_created(state, engine, c.new_surface_id);
        workspace_id
    };
    if origin.is_user()
        && let Some(ws) = engine.workspaces.get_mut(c.workspace_index)
    {
        ws.focused_pane = c.new_pane_id;
    }
    #[cfg(feature = "gui")]
    if origin.is_user()
        && let Some(workspace) = workspace_id
    {
        state.observe_tutorial_pane_split(workspace, c.original_pane_id, c.new_pane_id);
    }
}

#[cfg(feature = "gui")]
pub(crate) fn cascade_pane_closed(state: &mut dyn CascadeWindow, pane_id: u32) {
    state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneClosed { pane_id });
}

/// pane의 surface를 정리하고 GUI에서는 닫힘 알림도 등록한다.
// 이유: pane_id는 GUI 닫힘 알림에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "the pane id only feeds the gui-only pane.closed notice"
    )
)]
pub(crate) fn cascade_pane_closed_full(
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    pane_id: u32,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    reclaim_closed_surfaces(state, engine, cleanup_targets, is_user_close, None);
    #[cfg(feature = "gui")]
    cascade_pane_closed(state, pane_id);
}

#[cfg(feature = "gui")]
pub(crate) fn cascade_surface_created(
    state: &mut dyn CascadeWindow,
    engine: &CoreState,
    surface_id: u32,
) {
    let Some((tab_id, pane_id, workspace_id, kind)) = find_surface_location(engine, surface_id)
    else {
        return;
    };
    state.enqueue_host_event(crate::core::host_event::PendingHostEvent::SurfaceCreated {
        surface_id,
        kind,
        tab_id,
        pane_id,
        workspace_id,
        created_by_plugin: None,
    });
}

/// 현재 초기화된 트리에서 위치·종류를 찾는다. 포커스와 무관하며 없으면 None이다.
#[cfg(feature = "gui")]
fn find_surface_location(
    engine: &CoreState,
    surface_id: u32,
) -> Option<(u32, u32, u32, &'static str)> {
    for ws in &engine.workspaces {
        let workspace_id = ws.id;
        for pane_id in ws.pane_layout().all_pane_ids() {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            for tab in &pane.tabs {
                let Some(layout) = tab.layout_if_initialized() else {
                    continue;
                };
                if let Some(s) = layout.find_surface(surface_id) {
                    return Some((tab.id, pane_id, workspace_id, s.kind()));
                }
            }
        }
    }
    None
}

/// GUI의 탭·surface 생성 알림과 lifecycle 상태를 갱신한다.
// 이유: headless에는 실행할 본문이 없다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "every effect of this cascade is a notice with a consumer only in the gui build"
    )
)]
pub(crate) fn cascade_tab_created(
    state: &mut dyn CascadeWindow,
    engine: &CoreState,
    pane_id: u32,
    tab_id: u32,
    surface_id: u32,
) {
    #[cfg(feature = "gui")]
    {
        let workspace_id = engine
            .workspaces
            .iter()
            .find(|w| w.pane_layout().find_pane(pane_id).is_some())
            .map(|w| w.id);
        let kind = engine
            .find_pane_by_id(pane_id)
            .and_then(|p| p.tabs.iter().find(|t| t.id == tab_id))
            .and_then(|t| t.focused_surface_id())
            .and_then(|sid| engine.find_surface_by_id(sid))
            .map(|s| s.kind().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        if let Some(workspace_id) = workspace_id {
            state.enqueue_host_event(crate::core::host_event::PendingHostEvent::TabCreated {
                tab_id,
                pane_id,
                workspace_id,
                kind: kind.clone(),
            });
            state.lifecycle_baseline_insert_tab(tab_id, pane_id, workspace_id, kind);
        }
        cascade_surface_created(state, engine, surface_id);
    }
}

#[cfg(feature = "gui")]
pub(crate) fn cascade_tab_closed(state: &mut dyn CascadeWindow, tab_id: u32, pane_id: Option<u32>) {
    let Some(pane_id) = pane_id else {
        return;
    };
    state.enqueue_host_event(crate::core::host_event::PendingHostEvent::TabClosed {
        tab_id,
        pane_id,
    });
    state.lifecycle_baseline_remove_tab(tab_id);
}

/// 탭의 surface를 정리하고 GUI에서는 닫힘 알림과 lifecycle 상태도 갱신한다.
// 이유: tab_id와 pane_id는 GUI 닫힘 알림에만 쓰인다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        unused_variables,
        reason = "the tab and pane ids only feed the gui-only tab.closed notice"
    )
)]
pub(crate) fn cascade_tab_closed_full(
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    tab_id: u32,
    pane_id: Option<u32>,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    reclaim_closed_surfaces(state, engine, cleanup_targets, is_user_close, None);
    #[cfg(feature = "gui")]
    cascade_tab_closed(state, tab_id, pane_id);
}
