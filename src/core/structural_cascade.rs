//! 구조 변경 cascade — `Core::apply` 가 낸 split / tab / close 이벤트를 앱 상태에 반영한다.
//!
//! `Core::apply` 는 트리만 바꾸고 결과를 이벤트로 **반환**한다. 닫힌 surface 의 자원 회수
//! (PTY kill · 스크롤백 파일 · per-surface 인덱스 · memory scope · attach 점유 흔적), 활성
//! 포인터 보정, 빈 워크스페이스 재생성, 그리고 plugin 으로 나가는 통지 enqueue 는 여기서
//! 한다. 이 cascade 를 부르는 진입점은 셋이다 — 사용자 GUI 의 dispatcher
//! (`App::handle_core_event`), IPC 핸들러, 원격 mirror 가 forward 한 구조 op 의 실행
//! (`core::attach_runtime`). 셋이 같은 함수를 부르므로 진입점마다 회수가 갈리지 않는다.
//!
//! **왜 도메인 계층인가**: forward 실행은 `core` 안에 있다. cascade 가 `app` 에 있으면 그
//! 실행이 `app` 을 부르게 되고, 빌드 형태마다(gui / headless) 사본이 하나씩 필요했다 —
//! `app` 의 dispatcher 모듈은 두 빌드가 서로 다른 파일을 쓴다. 여기는 두 빌드가 **같은
//! 파일**을 컴파일하므로 함수 하나에 본문 하나다.
//!
//! **gui 와 headless 가 갈리는 지점은 `#[cfg(feature = "gui")]` 블록으로만 존재한다.**
//! 가르는 기준은 하나 — *그 효과에 headless 에서 소비자가 있는가*. plugin 으로 나가는
//! 통지(`surface.closed` lifecycle · `tab.*` / `pane.*` / `workspace.closed` /
//! `surface.created` host event)와 close 계측(C5 · `close_total`)과 튜토리얼 관찰은
//! gui 에만 소비자가 있어 gui 블록 안에 있다. 자원 회수·포인터 보정·재생성·사용자 origin
//! 포커스 이동은 소비자가 상태 자신이라 양쪽에 똑같이 돈다. 표는
//! `docs/architecture/close-sequence.md` "gui 와 headless 의 차이".

use crate::core::cascade_window::CascadeWindow;
use crate::core::intent::CascadeLevel;
use crate::core::origin::IntentOrigin;
use crate::core::{Core, CoreState};

/// `CoreEvent::SurfaceClosed` / `MoveSurfaceApplied` 가 공유하는 cascade 결과 —
/// 하나의 close 판정에서 나온 개념적 단위라 필드를 낱개로 끌고 다니지 않고 묶는다.
pub(crate) struct SurfaceCloseCascade {
    pub(crate) cascade_level: CascadeLevel,
    pub(crate) cleanup_targets: Vec<(u32, Option<String>)>,
    // 이유: 닫힌 tab 마다 `tab.closed` host event 를 내는 데만 쓰인다 — 그 통지의 소비자가
    // gui 뿐이라 headless 에서는 채워지기만 하고 안 읽힌다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "the tab.closed host event it feeds has a consumer only in the gui build"
        )
    )]
    pub(crate) closed_tab_ids: Vec<u32>,
    // 이유: 위 `closed_tab_ids` 와 같다(`pane.closed` host event 전용).
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "the pane.closed host event it feeds has a consumer only in the gui build"
        )
    )]
    pub(crate) closed_pane_ids: Vec<u32>,
    /// workspace 째 사라졌다면 그 **(인덱스, id)** — 짝을 타입이 강제한다.
    pub(crate) workspace_purged: Option<(usize, u32)>,
    pub(crate) workspaces_now_empty: bool,
    pub(crate) is_user_close: bool,
}

impl SurfaceCloseCascade {
    /// [`CoreEvent::SurfaceClosed`](crate::core::intent::CoreEvent::SurfaceClosed)
    /// → close cascade 입력. `None` = 이 이벤트가 `SurfaceClosed` 가 아니거나
    /// `closed=false`(대상을 못 찾았거나 닫을 수 없었다) — 어느 쪽이든 회수할 자원이 없다.
    ///
    /// `surface_id` 는 싣지 않는다 — 닫힌 surface 는 `cleanup_targets` 에 이미 들어 있고,
    /// cascade 는 그 목록만 본다.
    ///
    /// 부르는 곳은 둘이다 — gui dispatcher(`App::handle_core_event`)와 surface close 실행
    /// (`core::structural_exec::close_surface`, IPC 와 forward 가 함께 쓴다).
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

    /// [`CoreEvent::MoveSurfaceApplied`](crate::core::intent::CoreEvent::MoveSurfaceApplied)
    /// → close cascade 입력. 이동은 의미상 "B 닫힘 + A 옛자리 구조 cascade" 라 close 와
    /// 같은 cascade 를 탄다: `cleanup_targets` 는 닫히는 B 하나(PTY kill +
    /// `surface.closed`), 나머지 구조 필드는 A 의 옛 tab/pane/workspace 닫힘 정보다.
    /// A 의 surface 는 살아서 이동하므로 절대 cleanup 대상에 넣지 않는다.
    ///
    /// **한 자리에 두는 이유**: 이 매핑을 부르는 곳이 둘이다 — 로컬 dispatcher
    /// (`App::handle_core_event`)와 원격 forward 실행
    /// (`core::attach_runtime::execute_forwarded_structural_op`). 필드가 일곱이라
    /// 양쪽에 풀어 쓰면 variant 에 필드를 하나 더할 때 한쪽만 고쳐진 상태가 기본값이 된다.
    /// 이 파일은 두 빌드가 함께 컴파일하므로 기본 빌드 하나로 이 매핑의 모든 자리가 재진다.
    ///
    /// `None` = 이 이벤트가 `MoveSurfaceApplied` 가 아니거나 `moved=false` — 어느 쪽이든
    /// cascade 할 것이 없다. 둘을 갈라야 하는 호출자는 부르기 전에 variant 를 확인한다.
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

/// `CoreEvent::PaneSplit` 의 cascade 페이로드.
pub(crate) struct PaneSplitCascade {
    pub(crate) workspace_index: usize,
    // 이유: 아래 셋은 `pane.split` · `pane.created` · `surface.created` host event 와 튜토리얼
    // 관찰에만 쓰인다 — 그 소비자가 gui 뿐이라 headless 에서는 채워지기만 한다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui-only split notices and tutorial observation read it"
        )
    )]
    pub(crate) original_pane_id: u32,
    pub(crate) new_pane_id: u32,
    // 이유: 위 `original_pane_id` 와 같다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui-only surface.created notice reads it"
        )
    )]
    pub(crate) new_surface_id: u32,
    // 이유: 위 `original_pane_id` 와 같다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(dead_code, reason = "only the gui-only pane.split notice reads it")
    )]
    pub(crate) direction: crate::model::SplitDirection,
}

/// `CoreEvent::SurfaceClosed` 의 외부 cascade.
/// 1. 각 cleanup_target 에 `AppState::cleanup_surface` 호출
/// 2. cascade_level 별 host event (`tab.closed` / `pane.closed` / `workspace.closed`)
///    enqueue + baseline 동기화. `surface.closed` 자체는 별 큐
///    (`pending_lifecycle_events`) 가 처리하므로 여기선 안 다룸. — gui 만
/// 3. workspace_purged 가 Some 이면 memory scope purge — gui 만
/// 4. 같은 경우 활성 포인터(`active_workspace` · 카테고리 last-active)를 제거 위치
///    기준으로 보정 — 범위 초과 clamp 만으로는 사용자가 보던 것보다 **앞쪽**
///    workspace 가 빠질 때 인덱스가 유효한 채 다른 workspace 를 가리킨다(원칙 1)
///
/// 4 가 headless 에서도 도는 이유: 오늘의 headless 는 `active_workspace` 가 0 을 벗어나지
/// 못해(레이아웃 복원 미적용, `preset.apply` 는 focus 를 강제로 끄고, 워크스페이스 전환은
/// gui 전용 debug IPC 뿐) 이 분기의 결과가 범위 초과 clamp 와 같다. 그래도 같은 헬퍼를
/// 지나게 두는 것은, 포인터를 움직이는 headless 경로가 하나라도 생기는 순간 같은 불변식이
/// 빌드 형태에 따라 다르게 성립하지 않게 하려는 것이다. 근거
/// `docs/adr/0113-close-preserves-the-focused-target.md`.
// 이유: `workspace_purged` 의 id 쪽은 gui 전용 `after_workspace_removed` 만 쓴다.
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
    // C5 — 1. 각 cleanup_target 의 자원 회수 + lifecycle 통지
    #[cfg(feature = "gui")]
    let surfaces = c.cleanup_targets.len();
    reclaim_closed_surfaces(
        state,
        engine,
        c.cleanup_targets,
        c.is_user_close,
        Some("cascade"),
    );

    // 2. cascade_level 별 host event (`tab.closed` / `pane.closed`) enqueue +
    //    baseline 동기화. `surface.closed` 자체는 별 큐
    //    (`pending_lifecycle_events`) 가 처리하므로 여기선 안 다룸.
    #[cfg(feature = "gui")]
    {
        enqueue_closed_tab_events(state, &c.closed_tab_ids, &c.closed_pane_ids);
        enqueue_closed_pane_events(state, &c.closed_pane_ids);
    }

    // workspace 가 통째로 사라진 경우에만 도는 두 단계. 하나의 `Option<(usize, u32)>`
    // 라 "purge 는 했는데 포인터 보정은 안 했다" 가 성립하지 않는다.
    //    C4 — 3. memory scope purge + `workspace.closed` host event
    //    4. 인덱스 SoT 인 활성 포인터를 제거 위치 기준으로 보정
    // `workspace_purged` 는 Workspace level cascade 에서만 실린다 — 둘이 어긋나면
    // 보정이 조용히 건너뛰어져 사용자 화면이 밀리므로 debug 에서 고정한다.
    debug_assert_eq!(
        matches!(c.cascade_level, CascadeLevel::Workspace),
        c.workspace_purged.is_some(),
        "workspace level cascade 와 제거 위치는 함께 실려야 한다"
    );
    if let Some((removed_idx, workspace_id)) = c.workspace_purged {
        // 제거 후 공통 뒷정리는 초크포인트 하나가 한다 — 제거 경로 셋이 각자 쏘던
        // 때 인라인 cascade 가 `workspace.closed` 를 빠뜨렸다.
        #[cfg(feature = "gui")]
        state.after_workspace_removed(workspace_id, "cascade");
        state.fix_workspace_pointers_after_removal(removed_idx, engine.workspaces.len());
    }

    recreate_workspace_if_now_empty(core, state, engine, c.workspaces_now_empty);

    // close_total — workspace level cascade 일 때만. `Core::close_case_workspace`
    // 가 무장한 t0 을 여기서 소비하므로, tab/pane level cascade 는 무장 자체가
    // 없어 `None` 으로 빠진다.
    #[cfg(feature = "gui")]
    if let Some((t0, snapshot)) = crate::close_trace::take_cascade() {
        crate::close_trace::log_total(t0, surfaces, snapshot, "cascade");
    }
}

/// 닫힌 surface 들의 **자원 회수 + lifecycle 통지** — close cascade 셋
/// (`cascade_surface_closed` · `cascade_pane_closed_full` · `cascade_tab_closed_full`)이
/// 공유하는 한 자리다.
///
/// 셋이 각자 같은 루프를 들고 있었고, 그 사본들은 이미 갈라져 있었다 — surface 경로만
/// `cleanup_surface_traced` 로 C5 계측을 모았고 tab/pane 경로는 안 모았다. 루프가
/// 하나면 그 차이는 **인자 하나**(`trace`)로 드러나고, 새 단계를 더할 때 두 사본을
/// 찾아다니지 않는다.
///
/// `kind` 는 `cleanup_surface` **전에** 잡는다 — cleanup 직후엔 layout 에서 surface 가
/// 사라져 조회가 안 된다. cleanup_targets 의 sibling 이 lifecycle 큐에 빠짐없이 들어가야
/// plugin 쪽 per-surface 상태(예: 자식 registry)가 남지 않는다.
///
/// `trace`: `Some(path)` 면 C5(+C5a~C5d) 계측을 그 `path` 로 발화한다. `None` 이면
/// 누적만 하고 버린다 — 계측 발화 자체가 close 구간에 들어가므로 계측 대상 경로만 켠다.
///
/// headless 에서는 회수만 한다. lifecycle 큐를 비우는 주체(plugin manager / view)가 없어
/// enqueue 하면 큐가 프로세스 수명 동안 자라고, 계측 발화도 gui 의 close 계측 체계에 속한다.
///
/// 사용자(비-cascade) close 경로에는 같은 모양의 `AppState::cleanup_targets` 가 따로
/// 있다. 둘이 갈리는 지점은 `kind` 를 누가 구하느냐 하나다 — 저쪽은 호출자가 미리
/// resolve 한 값을 받고(트리에서 제거하기 **전**에 구할 수 있는 경로라 그 편이 정확하다),
/// 여기는 `Core::apply` 가 이미 트리를 건드린 뒤라 여기서 구한다.
// 이유: `is_user_close` 와 `trace` 는 gui 전용 lifecycle 통지와 계측 발화에만 쓰인다.
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

/// `cascade_surface_closed` 2 단계 (tab): 닫힌 tab 마다 `tab.closed` host event
/// enqueue + baseline 에서 제거. pane_id 는 close 후 못 찾으므로 closed_pane_ids
/// 의 첫 항목을 사용한다 (Tab level cascade 는 pane 안 닫혀 closed_pane_ids 비어
/// 있음 — 이때는 baseline 에서 lookup).
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

/// `cascade_surface_closed` 2 단계 (pane): 닫힌 pane 마다 `pane.closed` host
/// event enqueue.
#[cfg(feature = "gui")]
fn enqueue_closed_pane_events(state: &mut dyn CascadeWindow, closed_pane_ids: &[u32]) {
    for pane_id in closed_pane_ids {
        state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneClosed {
            pane_id: *pane_id,
        });
    }
}

/// `cascade_surface_closed` 마지막 단계: 마지막 surface 가 닫혀 workspaces 가
/// 비면 invariant 복구 위해 새 workspace 자동 생성. 사용자/에이전트/시스템 누구의
/// close 든 origin 분기 없이 동일 처리 — 빈 화면 redraw panic 방지가 목적. 옛
/// *세 호출처* (intent/surface.rs, ipc/close.rs,
/// pane.rs::close_surface_by_id_no_snapshot) 의 중복 분기를 단일 지점으로 통합.
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

/// `CoreEvent::SurfaceSplit` 의 외부 cascade. host event (`surface.created`)
/// 발화 + (User origin 이면) 해당 pane 의 active tab 의 focused_surface 를
/// new_surface_id 로 변경.
///
/// 포커스 이동은 headless 에서도 같은 조건으로 돈다 — 조건이 `User` origin 이고 그 origin 을
/// 세우는 발화점(단축키 · 메뉴 · 마우스)이 gui 에만 있어, headless 에서는 이 분기에 닿지
/// 않는다.
// 이유: `state` 는 gui 전용 `surface.created` host event 와 튜토리얼 관찰만 쓴다.
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

/// `CoreEvent::PaneSplit` 의 외부 cascade. host events (`pane.split` +
/// `pane.created`) 발화 + polling baseline 동기화 + (User origin 이면)
/// workspace 의 focused_pane 을 new_pane_id 로 변경.
///
/// 포커스 이동이 headless 에서도 같은 조건으로 도는 이유는 [`cascade_surface_split`] 과 같다.
// 이유: `state` 는 gui 전용 host event enqueue 와 튜토리얼 관찰만 쓴다.
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

/// `CoreEvent::PaneClosed` 의 외부 cascade. host event (`pane.closed`) enqueue.
#[cfg(feature = "gui")]
pub(crate) fn cascade_pane_closed(state: &mut dyn CascadeWindow, pane_id: u32) {
    state.enqueue_host_event(crate::core::host_event::PendingHostEvent::PaneClosed { pane_id });
}

/// `CoreEvent::PaneClosed` 의 full cascade — [`reclaim_closed_surfaces`] 로 자원 회수 +
/// `surface.closed` lifecycle enqueue, 그 뒤 `pane.closed` host event enqueue.
/// dispatcher (`App::dispatch_pane_closed_cascade`) 와 IPC handler 양쪽이 공유.
/// headless 는 자원 회수만 한다(통지의 소비자가 없다 — 모듈 문서).
// 이유: `pane_id` 는 gui 전용 `pane.closed` host event 만 쓴다.
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

/// 새 surface 생성 시 공통 host event 발화 — TabCreated / PaneSplit / SurfaceSplit
/// / WorkspaceCreated cascade 가 모두 사용. `surface_id` 의 위치 정보를 engine
/// 에서 lookup 해 `PendingHostEvent::SurfaceCreated` enqueue.
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

/// `surface_id` 의 host event 발화에 필요한 위치 + kind 를 모든 workspace 순회로
/// 찾는다 (focused 의존 없음). 못 찾으면 `None` — surface 가 아직 layout 에 안
/// 들어가 있거나 lazy init 인 케이스.
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

/// `CoreEvent::TabCreated` 의 외부 cascade. host events (`tab.created` +
/// `surface.created`) enqueue + polling baseline 동기화. workspace_id / kind 는
/// engine lookup. headless 에는 할 일이 없다 — 전부 통지다(모듈 문서).
// 이유: 본문 전체가 gui 전용 통지라 headless 에서는 인자를 하나도 안 읽는다.
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

/// `CoreEvent::TabClosed` 의 외부 cascade. host event (`tab.closed`) enqueue +
/// polling baseline 동기화. `pane_id` 가 `None` 이면 close 가 실패한 케이스
/// (find 못 함) — 아무것도 안 함.
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

/// `CoreEvent::TabClosed` 의 full cascade — [`reclaim_closed_surfaces`] 로 자원 회수 +
/// `surface.closed` lifecycle enqueue, 그 뒤 `tab.closed` host event enqueue +
/// baseline 동기화. dispatcher 와 IPC handler 양쪽이 공유. headless 는 자원 회수만 한다.
// 이유: `tab_id` 와 `pane_id` 는 gui 전용 `tab.closed` host event 만 쓴다.
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
