//! `Core` — 도메인 본체 + 단일 mutate 진입점 + 외부 자원 (port) 주입.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 본 Core 는 11 outbound port + preset_store 직속 보유만.
//! 도메인 데이터 (`CoreState`) 는 `crate::core::state` 에 — App.core_state
//! 가 main owner. D.3.C 의 도메인 마이그레이션으로 점진 흡수 예정.

pub(crate) mod agent;
pub(crate) mod attach;
pub(crate) mod attach_mesh_frames;
pub(crate) mod attach_readonly;
pub(crate) mod attach_runtime;
pub(crate) mod builder;
pub(crate) mod bulk_transfer;
pub(crate) mod capture_upload;
pub(crate) mod child_terminal;
pub(crate) mod file;
pub(crate) mod fs_list;
pub(crate) mod intent;
pub(crate) mod ipc_facade;
pub(crate) mod mesh_mirror;
pub(crate) mod pty_registry;
pub(crate) mod restore_rebuild;
pub(crate) mod session;
pub(crate) mod state;
pub(crate) mod terminal_store;

pub(crate) use state::{AttachMeshContextForward, CoreState, GuiAttachUserReq, PendingImageUpload};

use std::sync::{Arc, Mutex, OnceLock};

use intent::ProcessPtyOutcome;
use intent::{CoreEvent, DomainIntent};
use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::notification_sound::NotificationSoundPlayer;
use crate::ports::process::ProcessSpawner;

/// mirror(원격 attach client) 워크스페이스에서 구조 변경(split·new-tab·close·이동)이
/// 시도됐음을 나타내는 마커 에러. `Core::apply` 가 구조 `DomainIntent` 의 대상이
/// mirror 워크스페이스일 때 로컬 실행을 **거부**하며 반환한다 — 로컬 PTY spawn /
/// 로컬 트리 변경은 "workspace 전체가 remote" 불변식을 깨기 때문.
///
/// 호출자는 [`anyhow::Error::downcast_ref`] 로 이 타입을 식별해 (사용자 경로에서)
/// 차단 toast 를 띄운다. 구조 변경을 원격으로 forward 하는 2단계에서 이 지점이
/// forward 요청/응답으로 대체된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MirrorStructuralBlocked {
    /// 대상 mirror 워크스페이스 인덱스.
    pub workspace_index: usize,
    /// `true` 면 이 구조 op 를 원격으로 **forward** 하도록 큐에 넣었다(2단계). 이 경우
    /// 로컬 실행만 막고 차단 toast 는 띄우지 않는다(원격 실행 결과가 UX 를 결정 —
    /// 성공 시 무음, 실패 시 forward 실패 toast). `false` 면 forward 대상이 아닌 op
    /// (convert/move-surface 등)라 기존 차단 toast 를 띄운다.
    pub forwarded: bool,
}

impl std::fmt::Display for MirrorStructuralBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "structural change rejected: target belongs to a mirror (remote attach) workspace; \
             the operation must be performed on the remote instance"
        )
    }
}

impl std::error::Error for MirrorStructuralBlocked {}

/// mirror 구조 변경 forward 큐(`CoreState::pending_structural_forward`)의 원소.
/// `Core::apply` 는 origin 을 모르므로 항상 `user_triggered: false`(+ 빈 candidates)로
/// push 한다 — 이는 IPC/에이전트 호출과 동일하게 취급되는 안전한 기본값이다. origin 을
/// 아는 GUI 호출부(`intent::pane`/`intent::surface`/`intent::tab`, 그리고 origin 개념이
/// 아예 없이 항상 GUI 직접 호출인 `state::AppState::forward_mirror_structural`)가
/// 사후에 `user_triggered`를 뒤집거나(전자) 처음부터 `true`로 push한다(후자).
///
/// 08/09 두 이슈가 이 태그를 근거로 client-only focus 보정을 한다:
/// - **08**(새 리소스로 focus 이동): `user_triggered`가 true 인 new-tab/split 이
///   성공하면, 그 결과 delta 에서 새로 생긴 surface 로 focus 를 옮긴다.
/// - **09**(close 시 인접 대상 fallback): `close_focus_candidates`(로컬 surface id,
///   우선순위 순)를 담아두면, 닫힌 surface 가 focus 였던 경우(=기존 `restore_focus_
///   after_delta`가 복원할 대상을 잃는 경우) 첫 번째로 살아남은 후보로 focus 를
///   옮긴다. new-tab/split 등 close 가 아닌 op 은 항상 빈 벡터.
#[derive(Debug, Clone)]
pub(crate) struct PendingStructuralForward {
    pub(crate) op: crate::ipc::stream::StructuralOp,
    pub(crate) user_triggered: bool,
    pub(crate) close_focus_candidates: Vec<u32>,
}

impl PendingStructuralForward {
    fn agent(op: crate::ipc::stream::StructuralOp) -> Self {
        Self {
            op,
            user_triggered: false,
            close_focus_candidates: Vec::new(),
        }
    }
}

/// (04) 파일 피커 원격 `list_dir_request` id 시퀀스 — 프로세스 내 유일성만 필요
/// (capture 의 `upload_id` 시퀀스와 동일 근거).
static NEXT_LIST_DIR_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// 다음 list_dir_request id 발급. 파일 피커 popup wrapper
/// (`adapters::ui::popup::file_picker`)가 원격 조회를 트리거할 때 호출.
pub(crate) fn next_list_dir_request_id() -> u64 {
    NEXT_LIST_DIR_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// mirror 워크스페이스 원격 디렉토리 목록 forward 큐(`CoreState::pending_list_dir_forward`)
/// 의 원소. popup wrapper 가 (mirror 판별 후) push, App 이 `about_to_wait` 에서
/// drain 해 세션의 attach 채널로 `list_dir_request` 를 전송한다 — 구조 op forward
/// (`PendingStructuralForward`)/resize forward(`pending_resize_forward`)와 동형의
/// "popup/domain 이 큐에 push, App 레이어가 drain 해 실제 소켓 IO" 패턴.
#[derive(Debug, Clone)]
pub(crate) struct PendingListDirForward {
    pub(crate) local_ws_id: u32,
    pub(crate) request_id: u64,
    pub(crate) dir: String,
}

/// `core.apply(...)`가 mirror-block+forward 로 방금 push 한 **마지막** op 를 "사용자
/// GUI 조작 유래"로 표시한다(08). `err` 가 `forwarded=true`인 `MirrorStructuralBlocked`
/// 가 아니거나 `origin` 이 사용자가 아니면 no-op(기본 `false` 유지) — 다른 이유의
/// 실패로 큐에 아무것도 안 쌓였는데 엉뚱한 이전 op 를 잘못 표시하는 것을 막는다.
pub(crate) fn mark_last_forward_user_triggered(
    engine: &mut CoreState,
    err: &anyhow::Error,
    origin: &crate::intent::IntentOrigin,
) {
    let Some(blocked) = err.downcast_ref::<MirrorStructuralBlocked>() else {
        return;
    };
    if !blocked.forwarded || !origin.is_user() {
        return;
    }
    if let Some(last) = engine.pending_structural_forward.last_mut() {
        last.user_triggered = true;
    }
}

/// mirror 구조 `DomainIntent` → 원격 forward 할 [`StructuralOp`](crate::ipc::stream::StructuralOp).
/// anchor 는 **로컬** mirror surface id(App drain 이 세션 매핑으로 원격 id 로 치환).
/// pane/tab 대상 op 는 그 pane/tab 의 대표 surface(활성 탭의 focused surface)를 anchor 로
/// 삼아 원격이 자기 트리에서 pane/tab 을 resolve 하게 한다. forward 대상이 아닌
/// op(convert/move-surface — 재사용할 원격 IPC 핸들러 없음) 또는 anchor 를 못 찾으면
/// `None`(→ 기존 차단 유지).
fn build_mirror_forward_op(
    engine: &crate::core::CoreState,
    intent: &DomainIntent,
) -> Option<crate::ipc::stream::StructuralOp> {
    use crate::core::intent::DomainIntent as D;
    use crate::ipc::stream::{SplitAxis, StructuralOp};

    fn axis(d: &crate::model::SplitDirection) -> SplitAxis {
        match d {
            crate::model::SplitDirection::Horizontal => SplitAxis::Horizontal,
            crate::model::SplitDirection::Vertical => SplitAxis::Vertical,
        }
    }
    // pane 안 대표 surface(활성 탭의 focused surface) — pane/tab op 의 anchor.
    let pane_anchor = |pane_id: u32| -> Option<u32> {
        engine
            .find_pane_by_id(pane_id)
            .and_then(|p| p.tabs.get(p.active_tab))
            .and_then(|t| t.focused_surface_id())
    };
    let tab_anchor = |tab_id: u32| -> Option<u32> {
        for ws in &engine.workspaces {
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.id == tab_id {
                            return tab.focused_surface_id();
                        }
                    }
                }
            }
        }
        None
    };

    match intent {
        D::SplitSurface {
            target_surface_id,
            direction,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::SplitSurface {
            surface_id: *target_surface_id,
            direction: axis(direction),
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::SplitPane {
            target_pane_id,
            direction,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::SplitPane {
            anchor_surface_id: pane_anchor(*target_pane_id)?,
            direction: axis(direction),
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::CreateTab {
            pane_id,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::NewTab {
            anchor_surface_id: pane_anchor(*pane_id)?,
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::CloseSurface { surface_id, .. } => Some(StructuralOp::CloseSurface {
            surface_id: *surface_id,
        }),
        D::CloseTab { tab_id } => Some(StructuralOp::CloseTab {
            anchor_surface_id: tab_anchor(*tab_id)?,
        }),
        D::ClosePane { pane_id } => Some(StructuralOp::ClosePane {
            anchor_surface_id: pane_anchor(*pane_id)?,
        }),
        D::MoveTab {
            pane_id,
            from_index,
            to_index,
        } => Some(StructuralOp::MoveTab {
            anchor_surface_id: pane_anchor(*pane_id)?,
            from_index: *from_index,
            to_index: *to_index,
        }),
        // convert / move-surface 는 forward 미지원(차단 유지).
        _ => None,
    }
}

/// Helper: layout / tab_bar 기반으로 surface_id 별 *목표 grid (cols, rows)* 를
/// 수집. 본 helper 는 read-only 로 workspaces 만 순회한다 — `terminals` store 에는
/// 직접 접근하지 않으므로 caller 가 결과를 받아 `engine.terminals.get_mut` 로
/// resize 호출할 때 borrow 충돌이 없다.
#[cfg(feature = "gui")]
fn collect_terminal_resize_targets(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
    terminal_rect: crate::model::PhysicalRect,
    cell_width: f32,
    cell_height: f32,
) -> Vec<(u32, usize, usize)> {
    let tab_bar_h = state.tab_bar_height;
    let mut out = Vec::new();
    for ws in &engine.workspaces {
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, pane_rect) in pane_rects {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            let content_rect = crate::model::PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h).max(crate::model::PhysicalPx(1.0)),
            };
            for tab in &pane.tabs {
                let Some(layout) = tab.layout_opt.as_ref() else {
                    continue;
                };
                for (sid, rect) in layout.compute_rects(content_rect) {
                    let cols = ((rect.width.value() / cell_width.max(1.0)).floor() as usize).max(1);
                    let rows =
                        ((rect.height.value() / cell_height.max(1.0)).floor() as usize).max(1);
                    out.push((sid, cols, rows));
                }
            }
        }
    }
    out
}

/// Helper: tab 내 surface_id 에 해당하는 TerminalSurface 를 찾는다 (downcast).
fn terminal_surface_in_tab(
    tab: &crate::model::Tab,
    surface_id: u32,
) -> Option<&crate::model::TerminalSurface> {
    tab.layout_opt
        .as_ref()?
        .find_surface(surface_id)?
        .as_any()
        .downcast_ref::<crate::model::TerminalSurface>()
}

/// surface close cascade 의 Step 1 판정 결과 — C2(`apply_close_surface`) /
/// C3(`close_surface_by_id_inner`) 공유.
pub(crate) struct SurfaceCloseLocation {
    pub(crate) ws_idx: usize,
    pub(crate) pane_id: u32,
    pub(crate) tab_idx: usize,
    pub(crate) surface_is_sole_in_tab: bool,
    pub(crate) can_close_surface_in_group: bool,
}

/// surface 를 담은 ws/pane/tab 을 찾고 sole/split 판정. 순수 조회(뮤테이션 없음)라
/// C2/C3 공유. 못 찾으면 None (caller 는 not_found / false 로 귀결).
pub(crate) fn locate_surface_in_pane(
    engine: &crate::core::CoreState,
    surface_id: u32,
) -> Option<SurfaceCloseLocation> {
    let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(surface_id)?;
    let ws = &engine.workspaces[ws_idx];
    let pane = ws.pane_layout().find_pane(pane_id)?;
    let mut found_tab = None;
    for (i, tab) in pane.tabs.iter().enumerate() {
        if tab.contains_surface(surface_id) {
            found_tab = Some(i);
            break;
        }
    }
    let tab_idx = found_tab?;
    let tab = &pane.tabs[tab_idx];
    let surface_is_sole_in_tab;
    let can_close_surface_in_group;
    if tab.is_split() {
        surface_is_sole_in_tab = false;
        can_close_surface_in_group = !matches!(tab.layout(), crate::model::SurfaceLayout::Leaf(_));
    } else if tab.contains_surface(surface_id) {
        surface_is_sole_in_tab = true;
        can_close_surface_in_group = false;
    } else {
        return None;
    }
    Some(SurfaceCloseLocation {
        ws_idx,
        pane_id,
        tab_idx,
        surface_is_sole_in_tab,
        can_close_surface_in_group,
    })
}

/// surface 를 못 찾았을 때의 빈 cascade(`closed=false`). C2 전용.
pub(crate) fn surface_close_not_found(surface_id: u32) -> CoreEvent {
    CoreEvent::SurfaceClosed {
        surface_id,
        closed: false,
        cascade_level: crate::core::intent::CascadeLevel::Surface,
        cleanup_targets: vec![],
        closed_tab_ids: vec![],
        closed_pane_ids: vec![],
        workspace_id_purged: None,
        workspaces_now_empty: false,
    }
}

/// 도메인 본체. 10 outbound port (6 external + 4 internal) + preset_store 직속.
///
/// 도메인 데이터 (`crate::core::CoreState`) 는 본 struct 가 아닌
/// `App.core_state` 가 main owner — Phase D 진행 중의 *공존 layer*. D.3.C
/// 에서 점진 흡수.
#[allow(dead_code)] // 이유: Phase D 공존 layer scaffolding — App.core_state 가 main owner, D.3.C 흡수 대기.
pub(crate) struct Core {
    // ─── External ports (bin 안 정의, src/ports/) ───
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    clipboard: Arc<dyn ClipboardSystem>,
    process: Arc<dyn ProcessSpawner>,
    home: Arc<dyn HomeDirectory>,
    sound_player: Arc<dyn NotificationSoundPlayer>,

    // ─── Internal crate trait ports ───
    /// `Sync` 아님 (SQLite Connection 의 `RefCell` 캐시) — `Mutex` 보호.
    memory: Arc<Mutex<dyn MemoryStorage>>,
    themes: Arc<dyn ThemeStorage>,
    /// `&mut self` 메서드 (save/delete/rename) 가 있어 `Mutex` 보호.
    /// `preset_store` 와 *같은 allocation* (coerce 된 trait Arc).
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,

    /// Layout preset 디스크 캐시. 구체 Arc — MainView / PresetView 에 clone
    /// 으로 전달해 *공유 owner* 가 된다. `presets` (trait Arc) 와 같은 allocation.
    pub(crate) preset_store: Arc<Mutex<PresetStore>>,

    /// Host→plugin sync IPC dispatcher. Hub 가 IPC 서버를 띄운 직후 등록.
    /// runner thread 가 `claude.spawn` 등을 동기 호출할 때 사용.
    pub(crate) host_ipc_injector: Arc<OnceLock<crate::ipc::host_call::HostIpcInjector>>,

    /// Agent task runner registry — workspace 별 runner thread 의 시작/중단/상태.
    /// `agent.task_run` IPC + 통합 테스트가 사용.
    pub(crate) runner_registry: Arc<crate::core::agent::runner_thread::RunnerRegistry>,
}

impl Core {
    // ─── Domain wrapper methods ───
    //
    // sync 결과 반환이 필요한 mutate 는 본 wrapper 들로 노출 (handler 가 직접
    // 호출). Phase D 진행 중에는 wrapper 가 `engine` 도 함께 받아 그쪽을 mutate
    // 한다 — 도메인 데이터의 Core 흡수가 완료되면 `engine` 인자 제거 예정.

    /// Surface message 전송. 옛 `engine.send_message` 의 Core 진입점.
    pub(crate) fn send_surface_message(
        &mut self,
        engine: &mut crate::core::CoreState,
        from: u32,
        to: u32,
        content: String,
    ) -> u32 {
        engine.send_message(from, to, content)
    }

    /// Surface message 큐 read (peek/consume). 옛 `engine.read_messages` 의 Core 진입점.
    pub(crate) fn read_surface_messages(
        &mut self,
        engine: &mut crate::core::CoreState,
        sid: u32,
        from: Option<u32>,
        peek: bool,
    ) -> Vec<crate::state::SurfaceMessage> {
        engine.read_messages(sid, from, peek)
    }

    /// Surface message 큐 clear. 옛 `engine.clear_messages` 의 Core 진입점.
    pub(crate) fn clear_surface_messages(&mut self, engine: &mut crate::core::CoreState, sid: u32) {
        engine.clear_messages(sid);
    }

    // ─── Output observers (D.3.C.E.5) ───

    /// Observer 등록. 반환: 새 observer id.
    pub(crate) fn observer_register(
        &mut self,
        engine: &mut crate::core::CoreState,
        spec: crate::output_observer::ObserverSpec,
    ) -> Result<u64, crate::output_observer::ObserverError> {
        let memory = engine.memory.clone();
        let id = engine.observer_router.register(spec, memory)?;
        engine.sync_output_event_gates();
        Ok(id)
    }

    /// Observer 해제.
    pub(crate) fn observer_unregister(
        &mut self,
        engine: &mut crate::core::CoreState,
        observer_id: u64,
    ) -> Result<(), crate::output_observer::ObserverError> {
        engine.observer_router.unregister(observer_id)?;
        engine.sync_output_event_gates();
        Ok(())
    }

    /// Observer 목록 — read 인터페이스.
    pub(crate) fn observer_list(
        &self,
        engine: &crate::core::CoreState,
    ) -> Vec<crate::output_observer::ObserverInfo> {
        engine.observer_router.list()
    }

    /// 특정 observer 의 info — read 인터페이스.
    pub(crate) fn observer_info(
        &self,
        engine: &crate::core::CoreState,
        observer_id: u64,
    ) -> Option<crate::output_observer::ObserverInfo> {
        engine.observer_router.info(observer_id)
    }

    // ─── Hooks ───

    /// surface hook 등록. 반환: 새 hook id.
    ///
    /// `binding` 은 공유 훅 핸들러 레지스트리 참조([`HookBinding::Handler`]) 또는
    /// 하위호환 인라인 셸([`HookBinding::InlineShell`]). 배선/게이트는 IPC 핸들러
    /// (`hooks.rs`)에서 처리한다.
    pub(crate) fn register_surface_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        event: tasty_hooks::HookEvent,
        binding: tasty_hooks::HookBinding,
        once: bool,
    ) -> u64 {
        engine
            .hook_manager
            .add_hook(surface_id, event, binding, once)
    }

    /// surface hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        hook_id: u64,
    ) -> bool {
        engine.hook_manager.remove_hook(hook_id)
    }

    /// global hook 등록. 반환: 새 hook id.
    pub(crate) fn register_global_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        condition: crate::global_hooks::HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        engine.global_hook_manager.add(condition, command, label)
    }

    /// global hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_global_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        hook_id: u32,
    ) -> bool {
        engine.global_hook_manager.remove(hook_id)
    }

    /// surface 의 hook 들 중 event 매칭 시 fire — 발사된 hook id 들 반환.
    /// AppState 의 enqueue_host_event 는 호출처 (handler) 에서 처리.
    ///
    /// S9: 각 발사 훅의 바인딩을 `hook_handler::trigger` 로 실행한다(레지스트리 조회
    /// + `source` 게이트 + ShellCommand/IpcSequence 분기). IpcSequence 핸들러 실행에
    /// 필요한 IPC injector 는 main-thread 의 `host_ipc_injector` 에서 얻는다.
    pub(crate) fn fire_surface_hooks(
        &mut self,
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        events: &[tasty_hooks::HookEvent],
    ) -> Vec<u64> {
        let fired = engine.hook_manager.check_and_fire(surface_id, events);
        let injector = self.host_ipc_injector.get().cloned();
        let mut ids = Vec::with_capacity(fired.len());
        for f in &fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                surface_id,
            );
            ids.push(f.hook_id);
        }
        ids
    }

    // ─── Approval (휴먼 핸드오프) ───

    /// approval 요청 생성. 옛 `engine.approval_store.request` 의 Core 진입점.
    pub(crate) fn request_approval(
        &mut self,
        engine: &mut crate::core::CoreState,
        req: tasty_approval::ApprovalRequest,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.request(req)
    }

    /// approval 응답 적용. 옛 `engine.approval_store.respond` 의 Core 진입점.
    pub(crate) fn respond_approval(
        &mut self,
        engine: &mut crate::core::CoreState,
        req_id: &tasty_approval::ApprovalId,
        choice: String,
        by: tasty_approval::Responder,
        comment: Option<String>,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.respond(req_id, choice, by, comment)
    }

    /// approval 취소. 옛 `engine.approval_store.cancel` 의 Core 진입점.
    pub(crate) fn cancel_approval(
        &mut self,
        engine: &mut crate::core::CoreState,
        req_id: &tasty_approval::ApprovalId,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.cancel(req_id)
    }

    // ─── Memory (영속 store) ───

    /// Memory port 의 Arc clone. AppState 등 *Core 인자가 cascade 로 도달하지
    /// 못하는 표면* (UI popup draw_fn, state cleanup 등) 에 inject 해 동일
    /// allocation 의 port 를 공유시킨다.
    pub(crate) fn memory_arc(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        self.memory.clone()
    }

    /// Clipboard port 의 Arc clone. `memory_arc` 와 동일 목적 — cascade 로 도달하지
    /// 못하는 표면(스크린샷 캡처 워커 스레드, attach 서버측 원격 클립보드 기록)에
    /// inject 한다.
    pub(crate) fn clipboard_arc(&self) -> Arc<dyn ClipboardSystem> {
        self.clipboard.clone()
    }

    // ─── Host→plugin sync IPC dispatch ───

    /// Hub 가 IPC 서버를 시작한 직후 1회 호출. 두 번째 호출 부터는 무시.
    pub(crate) fn set_host_ipc_injector(&self, injector: crate::ipc::host_call::HostIpcInjector) {
        if self.host_ipc_injector.set(injector).is_err() {
            tracing::warn!("host_ipc_injector already initialized");
        }
    }

    /// runner thread 가 plugin IPC 메서드를 동기 호출. injector 미초기화 시 Err.
    #[allow(dead_code)] // 이유: Core 흡수 layer(D.3.C) — runner thread plugin IPC dispatch 경로 미배선.
    pub(crate) fn host_dispatch_plugin_method(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        let inj = self
            .host_ipc_injector
            .get()
            .ok_or_else(|| "host IPC injector not initialized".to_string())?;
        inj.dispatch(method, params, timeout)
    }

    /// Arc<OnceLock<HostIpcInjector>> 의 사본. 메인 스레드가 아닌 별도 스레드가
    /// 자체적으로 host IPC 를 재주입할 때 쓴다 — `terminal.tell`/`terminal.spawn`
    /// 의 제출 `\r` ack 대기 스레드(`adapters::ipc::handler::terminal`)가 소비.
    pub(crate) fn host_ipc_injector_arc(
        &self,
    ) -> Arc<OnceLock<crate::ipc::host_call::HostIpcInjector>> {
        self.host_ipc_injector.clone()
    }

    // ─── Agent task runner ───

    /// runner thread 가 사용할 컨텍스트를 조립. CoreState 의 agent_seq 를 함께.
    pub(crate) fn runner_context(
        &self,
        engine: &crate::core::CoreState,
    ) -> crate::core::agent::runner_host::RunnerContext {
        crate::core::agent::runner_host::RunnerContext {
            memory: self.memory.clone(),
            agent_seq: engine.agent_seq.clone(),
            host_ipc: self.host_ipc_injector.clone(),
            task_waker_hub: engine.task_waker_hub.clone(),
        }
    }

    pub(crate) fn agent_runner_registry(
        &self,
    ) -> Arc<crate::core::agent::runner_thread::RunnerRegistry> {
        self.runner_registry.clone()
    }

    /// Memory store 의 lock 안에서 함수를 실행한다. Mutex poisoning 시
    /// poison 해제 후 inner 사용 (host 부팅이 store 의 Arc 를 항상 inject
    /// 하므로 None 반환 분기는 없다 — 호출처는 `Result<R, _>` 만 처리하면 된다).
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard = match self.memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut *guard)
    }

    // Layout persistence wrapper (옛 `Core::save_layout` / `Core::restore_layout`)
    // 은 D.3.C.D.4 에서 `DomainIntent::SaveLayoutNow` /
    // `ApplyPendingLayoutRestore` 경로로 통합되어 제거됨.

    // ─── Notification sound (OS-level beep) ───

    /// Notification sound player port 참조. cascade 가 `settings.notification.sound`
    /// gate 통과 시 `play()` 호출.
    #[cfg(feature = "gui")]
    pub(crate) fn sound_player(&self) -> &Arc<dyn NotificationSoundPlayer> {
        &self.sound_player
    }

    // ─── PTY pipeline (D.3.C.C.6 / .8) — system loop wrapper ───
    //
    // 본 wrapper 들은 *system loop* (event_handler / about_to_wait / redraw /
    // busy poll) 의 PTY drain · resize · busy 갱신 경로의 Core 진입점.
    // *Intent 아님* — PTY 출력 / OS 타이머 / window resize 같은 *외부 trigger*
    // 에서 호출.
    //
    // 시그니처 정책:
    // - `process_pty_output` / `process_all_pty_output` 은 `&mut self` 메서드 —
    //   ClipboardSet (OSC 52) 처리 시 self.clipboard port 접근이 필요하다.
    //   App 컨텍스트에서만 호출되므로 (event_handler / about_to_wait) self.core
    //   접근 가능.
    // - 나머지 4 wrapper (flush / force_flush / resize_all / update_busy) 는
    //   port 접근이 없어 associated fn — MainView 안 redraw / shortcuts
    //   dispatch 에서 `self.core` 없이 호출 가능하다.

    /// 특정 surface 의 PTY 출력 drain + TerminalEvent → CoreEvent 변환.
    /// observer_router (OutputAppended) / command_index (PromptBoundary) /
    /// 시스템 clipboard (OSC 52) 의 부수효과는 본 함수가 직접 처리. 나머지
    /// terminal event 는 outcome.events 로 cascade dispatcher 에 전달.
    // headless 메인 루프는 `process_all_pty_output` 만 사용한다 — 단일 surface 변형은
    // gui event_handler 의 targeted polling 전용이라 headless 에선 dead.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) fn process_pty_output(
        &mut self,
        engine: &mut crate::core::CoreState,
        surface_id: u32,
    ) -> ProcessPtyOutcome {
        let processed = engine.process_surface(surface_id);
        let events = self.drain_terminal_events(engine);
        ProcessPtyOutcome { events, processed }
    }

    /// 모든 workspace 의 모든 terminal 을 drain + 변환. 반환: cascade 가 처리할
    /// CoreEvent 목록 + 어느 surface 든 데이터 drain 했는지.
    pub(crate) fn process_all_pty_output(
        &mut self,
        engine: &mut crate::core::CoreState,
    ) -> ProcessPtyOutcome {
        let processed = engine.process_all();
        let events = self.drain_terminal_events(engine);
        ProcessPtyOutcome { events, processed }
    }

    /// `engine.collect_events()` 결과를 CoreEvent 로 변환. observer_router /
    /// command_index / system clipboard 의 *직접 부수효과* 는 본 함수가 처리하고,
    /// cascade 가 필요한 event 만 Vec<CoreEvent> 로 반환.
    fn drain_terminal_events(&mut self, engine: &mut crate::core::CoreState) -> Vec<CoreEvent> {
        use tasty_terminal::TerminalEventKind;
        let raw = engine.collect_events();
        let mut out = Vec::with_capacity(raw.len());
        for ev in raw {
            let sid = ev.surface_id;
            match ev.kind {
                TerminalEventKind::OutputAppended { text } => {
                    engine.observer_router.dispatch_text(sid, &text);
                }
                TerminalEventKind::PromptBoundary { phase, payload } => {
                    let mem = engine.memory.clone();
                    if let Some(cap) = engine.command_index.on_boundary(&mem, sid, phase, &payload)
                    {
                        use crate::engine::command_index::CommandCapEvent;
                        let (title, body) = match cap {
                            CommandCapEvent::SoftWarn { count, .. } => (
                                crate::i18n::t("command_index.cap.soft.title").to_string(),
                                crate::i18n::t_fmt(
                                    "command_index.cap.soft.body",
                                    &count.to_string(),
                                ),
                            ),
                            CommandCapEvent::HardBlocked { .. } => (
                                crate::i18n::t("command_index.cap.hard.title").to_string(),
                                crate::i18n::t("command_index.cap.hard.body").to_string(),
                            ),
                        };
                        out.push(CoreEvent::TerminalNotification {
                            surface_id: sid,
                            title,
                            body,
                        });
                    }
                }
                TerminalEventKind::ClipboardSet(text) => {
                    if let Err(e) = self.clipboard.write_text(&text) {
                        tracing::warn!("OSC 52 clipboard write failed: {e}");
                    }
                    out.push(CoreEvent::TerminalClipboardSet { surface_id: sid });
                }
                TerminalEventKind::ClipboardQuery => {
                    // OSC 52 read query. Security gate: off by default so an
                    // arbitrary (possibly remote/untrusted) program cannot
                    // silently read the local clipboard. When off, send nothing —
                    // no reply byte must leave the host. Handled here (not via a
                    // cascade) like the ClipboardSet write, since both need the
                    // `self.clipboard` port together with the terminal engine.
                    let allow = engine.settings.general.allow_clipboard_read;
                    // Only touch the clipboard when allowed (default off → never read).
                    let clip = if allow {
                        match self.clipboard.read_text() {
                            Ok(t) => Some(t),
                            Err(e) => {
                                tracing::warn!("OSC 52 clipboard read failed: {e}");
                                None
                            }
                        }
                    } else {
                        None
                    };
                    if let Some(reply) = osc52_clipboard_read_reply(allow, clip.as_deref())
                        && let Some(terminal) = engine.find_terminal_by_id_mut(sid)
                    {
                        terminal.send_bytes(&reply);
                    }
                }
                TerminalEventKind::Notification { title, body } => {
                    out.push(CoreEvent::TerminalNotification {
                        surface_id: sid,
                        title,
                        body,
                    });
                }
                TerminalEventKind::BellRing => {
                    out.push(CoreEvent::TerminalBellRing { surface_id: sid });
                }
                TerminalEventKind::TitleChanged(title) => {
                    out.push(CoreEvent::TerminalTitleChanged {
                        surface_id: sid,
                        title,
                    });
                }
                TerminalEventKind::CwdChanged(_cwd) => {
                    out.push(CoreEvent::TerminalCwdChanged { surface_id: sid });
                }
                TerminalEventKind::ProcessExited => {
                    out.push(CoreEvent::TerminalProcessExited { surface_id: sid });
                }
            }
        }
        out
    }

    /// throttle 적용 PTY resize flush. 옛 `engine.flush_all_pty_resizes()` 의 진입점.
    /// 반환: 여전히 pending 이 남았는지 (redraw 재요청 신호).
    #[cfg(feature = "gui")]
    pub(crate) fn flush_pty_resizes(engine: &mut crate::core::CoreState) -> bool {
        engine.flush_all_pty_resizes()
    }

    /// 모든 workspace 의 모든 terminal 을 layout 에 맞춰 resize. 옛
    /// `state.resize_all(engine, ...)` 의 진입점. tab_bar_height 가 AppState 에
    /// 있어 `state` 도 인자로 받는다 (도메인 흡수 후 제거 예정).
    ///
    /// D.3.E.4 이후 TerminalSurface 는 id-marker 라 `Surface::resize_all` 은
    /// no-op. Terminal 본체는 `engine.terminals` (TerminalStore) 가 owner 이므로
    /// 여기서 직접 store 를 두드려 resize 한다.
    #[cfg(feature = "gui")]
    pub(crate) fn resize_all_terminals(
        state: &crate::state::AppState,
        engine: &mut crate::core::CoreState,
        terminal_rect: crate::model::PhysicalRect,
        cell_width: f32,
        cell_height: f32,
    ) {
        let targets =
            collect_terminal_resize_targets(state, engine, terminal_rect, cell_width, cell_height);
        for (sid, cols, rows) in targets {
            // hard-점유된 surface(원격 client 가 mirror 로 구동 중인 서버측 실제 PTY)는
            // client-driven geometry(ADR-0045) — 점유 client 가 유일 구동자다. 이 host
            // 창의 레이아웃 sweep 이 원격 창 grid 로 되돌리면 client 의 ClientResize 가
            // 무력화되어 mirror 가 host 창 크기에 고정(레터박스)된다. 따라서 점유 중인
            // surface 는 여기서 skip 하고, 오직 `apply_attached_workspace_resize`(holder
            // 검증 후 client 요청 크기 적용)만 이 surface 의 grid 를 설정하게 한다. detach 로
            // lock 이 풀리면 다음 sweep 부터 host 창이 다시 구동한다(원복).
            if engine.attach.is_hard_occupied(sid) {
                continue;
            }
            if let Some(t) = engine.terminals.get_mut(sid) {
                // mirror(detached) 터미널은 client-driven geometry(ADR-0045):
                // 로컬 pane 목표 grid 를 로컬에 **직접 적용하지 않고**(로컬 grid 는
                // server 의 `Resize` echo 로만 갱신 → 원격 reflow 전 잘못된 grid 에
                // 바이트가 재생되는 desync 방지) 원격 PTY 를 그 크기로 구동하도록
                // forward 큐에 넣는다. 목표가 현재 mirror grid 와 같으면(정상상태)
                // enqueue 하지 않는다 — 전송할 변화가 없다. (transient 중복은
                // dispatch 의 세션 last-forwarded dedup 이 흡수한다.)
                if t.is_detached() {
                    if t.cols() != cols || t.rows() != rows {
                        engine.pending_resize_forward.insert(sid, (cols, rows));
                    }
                    continue;
                }
                t.resize(cols, rows);
            }
        }
    }

    /// busy surface 집합 갱신. 옛 `engine.refresh_busy_surfaces()` 의 진입점.
    /// `AppEvent::BusyPoll` (1Hz 타이머) 에서 호출. 반환: 집합이 변했는지
    /// (window mark_dirty 결정 신호).
    #[cfg(feature = "gui")]
    pub(crate) fn update_busy_surfaces(engine: &mut crate::core::CoreState) -> bool {
        engine.refresh_busy_surfaces()
    }

    /// 도메인 변경의 단일 진입점. handler 가 발행한 `DomainIntent` 를 받아
    /// 결과 이벤트 목록을 반환. Phase D 진행 중 — variant 추가 시 본 match 도 채움.
    ///
    /// `engine` 인자: 발화 대상 engine. 현재 *이벤트만 발행* 패턴인 variant
    /// 들은 인자를 사용하지 않으나 (점진적 흡수 진행 중), workspace.create
    /// 처럼 *결과 정보가 필요한* variant 는 본 메서드 안에서 직접 mutate 후
    /// event 에 결과를 담아 반환한다. CreateWorkspace 분기만 engine 을
    /// 사용하므로 rustc 는 unused 경고를 내지 않는다.
    pub(crate) fn apply(
        &mut self,
        engine: &mut crate::core::CoreState,
        intent: DomainIntent,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        // mirror(원격 attach client) 워크스페이스 누출 차단 — 그 안의 구조 변경은
        // 로컬에서 실행하지 않는다(로컬 PTY spawn / 트리 변경 금지). 사용자 단축키·
        // 에이전트 IPC 어느 진입 경로든 여기(단일 mutate 진입점)로 수렴하므로 한 곳에서
        // 막는다. 구조와 무관한 intent 는 통과. (2단계에서 이 지점이 원격 forward 로 대체.)
        if let Some(workspace_index) = engine.mirror_workspace_index_for_structural(&intent) {
            // 2단계: 로컬 실행은 여전히 막되(불변식 유지), forward 가능한 op 는 원격에
            // 넘기도록 큐에 넣는다. anchor 는 아직 로컬 surface id — App drain 이 세션
            // 매핑으로 원격 id 로 치환해 전송한다. forward 불가 op(convert/move-surface)는
            // None → 기존 차단 toast.
            let forwarded = match build_mirror_forward_op(engine, &intent) {
                Some(op) => {
                    engine
                        .pending_structural_forward
                        .push(PendingStructuralForward::agent(op));
                    true
                }
                None => false,
            };
            return Err(anyhow::Error::new(MirrorStructuralBlocked {
                workspace_index,
                forwarded,
            }));
        }
        match intent {
            // Phase D 진행 중 — 본 stub 들은 *이벤트만 발행*. cascade
            // (Theme apply / Scrollback limit / clipboard max / notification
            // coalesce 등) 는 후속 sub-step (호출처 전환과 함께) 에서 통합.
            DomainIntent::UpdateSettings(new_settings) => {
                Ok(vec![CoreEvent::SettingsUpdated(new_settings)])
            }
            DomainIntent::PushNotification {
                ws_id,
                surface_id,
                title,
                body,
                source,
            } => Ok(vec![CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source,
            }]),
            DomainIntent::MarkNotificationRead { id } => {
                Ok(vec![CoreEvent::NotificationReadRequested { id }])
            }
            DomainIntent::MarkAllNotificationsRead => {
                Ok(vec![CoreEvent::AllNotificationsReadRequested])
            }
            DomainIntent::SurfaceCwdChanged { surface_id } => {
                Ok(vec![CoreEvent::SurfaceCwdChanged { surface_id }])
            }
            DomainIntent::SetTerminalMark { surface_id } => {
                Ok(vec![CoreEvent::TerminalMarkSet { surface_id }])
            }
            DomainIntent::SurfaceCompletion { surface_id } => {
                Ok(vec![CoreEvent::SurfaceCompletionRequested { surface_id }])
            }
            DomainIntent::CreateWorkspace {
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
                category,
            } => self.apply_create_workspace(
                engine,
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
                category,
            ),
            DomainIntent::UpdateWorkspaceMeta {
                workspace_id,
                name,
                subtitle,
                description,
            } => {
                self.apply_update_workspace_meta(engine, workspace_id, name, subtitle, description)
            }
            DomainIntent::MoveWorkspace {
                from_index,
                to_index,
            } => Ok(vec![
                self.apply_move_workspace(engine, from_index, to_index),
            ]),
            DomainIntent::CreateTab {
                pane_id,
                cwd,
                kind,
                name,
                surface_params,
            } => Self::apply_create_tab(engine, pane_id, cwd, kind, name, surface_params),
            DomainIntent::CloseTab { tab_id } => Ok(vec![Self::apply_close_tab(engine, tab_id)]),
            DomainIntent::MoveTab {
                pane_id,
                from_index,
                to_index,
            } => Ok(vec![Self::apply_move_tab(
                engine, pane_id, from_index, to_index,
            )]),
            DomainIntent::AdoptTerminal { pane_id, pty_id } => {
                Self::apply_adopt_terminal(engine, pane_id, pty_id)
            }
            DomainIntent::SplitPane {
                target_pane_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => {
                Self::apply_split_pane(engine, target_pane_id, direction, cwd, kind, surface_params)
            }
            DomainIntent::SplitSurface {
                target_surface_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => Self::apply_split_surface(
                engine,
                target_surface_id,
                direction,
                cwd,
                kind,
                surface_params,
            ),
            DomainIntent::ClosePane { pane_id } => {
                Ok(vec![Self::apply_close_pane(engine, pane_id)])
            }
            DomainIntent::CloseSurface {
                surface_id,
                save_snapshot,
            } => Ok(vec![Self::apply_close_surface(
                engine,
                surface_id,
                save_snapshot,
            )]),
            DomainIntent::ConvertSurface { surface_id, target } => {
                Ok(vec![Self::apply_convert_surface(
                    engine, surface_id, target,
                )])
            }
            DomainIntent::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => Ok(vec![Self::apply_move_surface(
                engine,
                source_surface_id,
                target_surface_id,
            )]),
            DomainIntent::SendToSurface {
                surface_id,
                payload,
            } => Ok(vec![Self::apply_send_to_surface(
                engine, surface_id, payload,
            )]),
            DomainIntent::RespawnTerminal { surface_id, cwd } => {
                Ok(vec![Self::apply_respawn_terminal(engine, surface_id, cwd)])
            }
            DomainIntent::RestoreClosedItem { target_pane_id } => {
                Ok(vec![Self::apply_restore_closed_item(
                    engine,
                    target_pane_id,
                )])
            }
            DomainIntent::UpdateTabName { surface_id, name } => {
                Ok(vec![Self::apply_update_tab_name(engine, surface_id, name)])
            }
            DomainIntent::SaveLayoutNow {
                active_workspace,
                force,
            } => Ok(vec![Self::apply_save_layout_now(
                engine,
                active_workspace,
                force,
            )]),
            DomainIntent::ApplyPendingLayoutRestore => {
                Ok(vec![Self::apply_apply_pending_layout_restore(engine)])
            }
            DomainIntent::DispatchFile {
                target,
                depth,
                origin_surface_id,
                ignore_size_limit,
            } => {
                #[cfg(feature = "gui")]
                {
                    match engine.identify_worker.as_ref() {
                        Some(worker) => {
                            // request id not tracked.
                            let _id =
                                worker.spawn(target, depth, origin_surface_id, ignore_size_limit);
                        }
                        None => {
                            tracing::warn!(
                                target = %target.display(),
                                "DispatchFile: identify_worker not injected — drop",
                            );
                        }
                    }
                }
                #[cfg(not(feature = "gui"))]
                {
                    // headless: no identify_worker.
                    let _ = (engine, target, depth, origin_surface_id, ignore_size_limit);
                    tracing::warn!("DispatchFile dropped in headless build");
                }
                Ok(vec![])
            }
        }
    }

    /// `DomainIntent::SaveLayoutNow` 본문. settings + debounce + force gate 를
    /// 통과하면 디스크에 저장 + `layout_dirty.clear()`. 옛 `App::flush_layout_persistence`
    /// 의 조건 분기 + 옛 `Core::save_layout` wrapper 본문을 흡수.
    fn apply_save_layout_now(
        engine: &mut crate::core::CoreState,
        active_workspace: usize,
        force: bool,
    ) -> CoreEvent {
        let g = &engine.settings.general;
        let should_save = if force {
            g.restore_layout && (engine.layout_dirty.is_dirty() || g.restore_surface_content)
        } else {
            g.restore_layout && engine.layout_dirty.should_flush()
        };
        if !should_save {
            return CoreEvent::LayoutSaved { saved: false };
        }
        crate::engine::layout_persistence::save_to_disk(engine, active_workspace);
        engine.layout_dirty.clear();
        CoreEvent::LayoutSaved { saved: true }
    }

    /// `DomainIntent::ApplyPendingLayoutRestore` 본문. engine 의
    /// `pending_layout_restore` 를 take 해 `SavedLayout::restore` 호출. 성공 시
    /// `restored_active_workspace` 도 take 해 CoreEvent payload 로 caller 에게 넘김.
    /// caller (window_lifecycle.rs::create_app_state) 가 결과 받아
    /// `state.switch_workspace` 수행.
    fn apply_apply_pending_layout_restore(engine: &mut crate::core::CoreState) -> CoreEvent {
        let Some(saved) = engine.pending_layout_restore.take() else {
            return CoreEvent::LayoutRestored {
                restored: false,
                active_workspace: None,
            };
        };

        // Fix C: 복원이 surface_id 를 발급하기 *전에* 카운터 floor 를 memory.db 의
        // 최대 stale Scope::Surface id 위로 올린다. surface_meta 는 영속되지만
        // surface_id 는 매 실행 재발급되므로, 이래야 복원 surface 자체가 재사용 id
        // (=stale 메타 보유)와 겹치지 않아 capture 가 남의 restore.command 를 읽지 않는다.
        {
            let mut guard = match engine.memory.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let mem_max = crate::surface_meta::SurfaceMetaStore::max_surface_id(&mut *guard);
            engine.next_ids.bump_surface_floor(mem_max + 1);
        }

        if !saved.restore(engine) {
            return CoreEvent::LayoutRestored {
                restored: false,
                active_workspace: None,
            };
        }

        // Fix A: 복원으로 확정된 live id 외 모든 Surface scope 를 정리한다. Fix C 가
        // 충돌은 막지만 죽은 scope 를 지우지는 않으므로, 강제 종료 등으로 graceful
        // close 가 호출되지 못해 남은 stale 메타가 무한 누적되는 것을 끊는다.
        {
            let live: std::collections::HashSet<u32> = engine
                .workspaces
                .iter()
                .flat_map(|ws| ws.all_surface_ids())
                .collect();
            let mut guard = match engine.memory.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let removed =
                crate::surface_meta::SurfaceMetaStore::purge_dead_surfaces(&mut *guard, &live);
            if removed > 0 {
                tracing::info!(
                    "surface_meta GC: purged {removed} dead surface scope(s) on restore"
                );
            }
        }

        let active = engine.restored_active_workspace.take();
        CoreEvent::LayoutRestored {
            restored: true,
            active_workspace: active,
        }
    }

    /// `DomainIntent::RestoreClosedItem` 본문. closed_items stack pop → kind 별
    /// rebuild + engine attach. AppState 의존 부분 (active_workspace 변경) 은
    /// cascade 가 처리하므로 본 함수는 *engine mutate* 만.
    fn apply_restore_closed_item(
        engine: &mut crate::core::CoreState,
        target_pane_id: Option<u32>,
    ) -> CoreEvent {
        use crate::core::intent::RestoredKind;
        use crate::core::restore_rebuild;
        use crate::model::Surface;
        use crate::model::Tab;
        use crate::model::Workspace;
        use crate::model::closed_item::ClosedItem;

        let nothing = || CoreEvent::ClosedItemRestored {
            restored: false,
            kind: RestoredKind::Nothing,
        };

        let Some(item) = engine.closed_items.pop() else {
            return nothing();
        };

        let kind = match item {
            ClosedItem::Surface { surface, tab_name } => {
                let Some(node) = restore_rebuild::rebuild_surface_node(engine, surface) else {
                    return nothing();
                };
                let Some(pane_id) = target_pane_id else {
                    return nothing();
                };
                let tab_id = engine.next_ids.next_tab();
                let surface_box: Box<dyn Surface> = Box::new(node);
                let tab = Tab::new_with_surface(tab_id, tab_name, surface_box);
                if !push_tab_to_pane(engine, pane_id, tab) {
                    return nothing();
                }
                RestoredKind::TabIntoPane { pane_id }
            }
            ClosedItem::Tab(closed_tab) => {
                let Some(result) = restore_rebuild::rebuild_surface(engine, closed_tab.panel)
                else {
                    return nothing();
                };
                let Some(pane_id) = target_pane_id else {
                    return nothing();
                };
                let tab_id = engine.next_ids.next_tab();
                let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
                let tab = result.into_tab(tab_id, name);
                if !push_tab_to_pane(engine, pane_id, tab) {
                    return nothing();
                }
                RestoredKind::TabIntoPane { pane_id }
            }
            ClosedItem::Workspace {
                name,
                subtitle,
                pane_layout,
                focused_pane,
                ..
            } => {
                let ws_id = engine.next_ids.next_workspace();
                let Some(pane_node) = restore_rebuild::rebuild_pane_node(engine, pane_layout)
                else {
                    return nothing();
                };
                let all_pane_ids = pane_node.all_pane_ids();
                let actual_focused = if all_pane_ids.contains(&focused_pane) {
                    focused_pane
                } else {
                    *all_pane_ids.first().unwrap_or(&0)
                };
                let ws = Workspace::from_restored(ws_id, name, subtitle, pane_node, actual_focused);
                engine.workspaces.push(ws);
                RestoredKind::Workspace {
                    new_ws_index: engine.workspaces.len() - 1,
                }
            }
        };

        engine.mark_layout_dirty();
        CoreEvent::ClosedItemRestored {
            restored: true,
            kind,
        }
    }

    /// `DomainIntent::UpdateTabName` 본문. surface_id 가 속한 tab 을 *모든*
    /// workspace 에서 검색 (포커스 독립) → `osc_title` 필드 set. explicit_name
    /// 은 건드리지 않는다 — 사용자가 직접 이름 지은 tab 보존.
    fn apply_update_tab_name(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        name: String,
    ) -> CoreEvent {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return CoreEvent::TabNameUpdated {
                surface_id,
                tab_id: None,
                skipped_explicit: false,
            };
        }
        for ws in &mut engine.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.all_surface_ids().contains(&surface_id) {
                            let tab_id = tab.id;
                            if tab.explicit_name.is_some() {
                                return CoreEvent::TabNameUpdated {
                                    surface_id,
                                    tab_id: Some(tab_id),
                                    skipped_explicit: true,
                                };
                            }
                            // 오직 그 탭의 *focused* surface 발화만 탭 제목에 반영한다.
                            // 병렬 surface 의 title 발화가 last-writer-wins 로 제목을
                            // 흔드는 flicker 방지 (cwd 경로 refresh_tab_display_name 와
                            // 동일 정책). SurfaceTitleChanged host event 는 상류
                            // cascade_terminal_title_changed 에서 이미 발화되므로
                            // 이 가드가 plugin 호환에 영향 없다.
                            if tab.focused_surface != surface_id {
                                return CoreEvent::TabNameUpdated {
                                    surface_id,
                                    tab_id: Some(tab_id),
                                    skipped_explicit: false,
                                };
                            }
                            tab.osc_title = Some(name);
                            return CoreEvent::TabNameUpdated {
                                surface_id,
                                tab_id: Some(tab_id),
                                skipped_explicit: false,
                            };
                        }
                    }
                }
            }
        }
        CoreEvent::TabNameUpdated {
            surface_id,
            tab_id: None,
            skipped_explicit: false,
        }
    }

    /// `DomainIntent::RespawnTerminal` 본문. 새 Terminal 생성 → engine.replace_terminal_by_id.
    fn apply_respawn_terminal(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        cwd: Option<std::path::PathBuf>,
    ) -> CoreEvent {
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);
        let new_terminal = match tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                surface_id,
                working_dir: cwd.as_deref(),
                initial_input: None,
            },
            waker,
        ) {
            Ok(t) => t,
            Err(e) => {
                return CoreEvent::TerminalRespawned {
                    surface_id,
                    error: Some(e.to_string()),
                };
            }
        };
        match engine.replace_terminal_by_id(surface_id, new_terminal) {
            Ok(()) => CoreEvent::TerminalRespawned {
                surface_id,
                error: None,
            },
            Err(e) => CoreEvent::TerminalRespawned {
                surface_id,
                error: Some(e.to_string()),
            },
        }
    }

    /// `DomainIntent::SendToSurface` 본문. ensure_surface_initialized → terminal
    /// lookup → send_bytes / send_key 분기.
    fn apply_send_to_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        payload: crate::core::intent::SendPayload,
    ) -> CoreEvent {
        // §2.4 서버 본인 입력 차단: attach 로 점유된 surface 는 서버 로컬 입력
        // (사용자 GUI 키 / IPC surface.send*) 이 PTY 에 닿지 못한다. client 경유
        // 입력은 단계 4 의 holder-검증 attach 채널로 들어와 이 경로를 우회한다.
        if engine.attach.is_hard_occupied(surface_id) {
            return CoreEvent::SurfaceSent {
                surface_id,
                sent: false,
            };
        }
        engine.ensure_surface_initialized(surface_id);
        let sent = if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
            match payload {
                crate::core::intent::SendPayload::Bytes(bytes) => {
                    terminal.send_bytes(&bytes);
                }
                crate::core::intent::SendPayload::Text(text) => {
                    terminal.send_key(&text);
                }
            }
            true
        } else {
            false
        };
        CoreEvent::SurfaceSent { surface_id, sent }
    }

    /// `DomainIntent::SplitPane` 본문. 4-phase borrow 분리.
    fn apply_split_pane(
        engine: &mut crate::core::CoreState,
        target_pane_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let ws_idx = engine
            .find_workspace_index_for_pane(target_pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not found", target_pane_id))?;

        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        // Phase 1: engine 의 불변 의존 추출
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(new_surface_id);

        // Phase 2: 새 pane 구성
        let new_pane = if is_terminal {
            let terminal = crate::model::Pane::spawn_terminal(
                new_surface_id,
                crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker,
                    working_dir: cwd.as_deref(),
                },
            )?;
            engine.terminals.insert(new_surface_id, terminal);
            crate::model::Pane::new_with_terminal_marker(new_pane_id, new_tab_id, new_surface_id)
        } else {
            let surface = engine.create_surface_via_registry(
                &kind,
                new_surface_id,
                cwd.as_deref(),
                &surface_params,
            )?;
            let name = crate::state::pane::default_tab_name_for_kind(
                &kind,
                &surface_params,
                engine.surface_registry.get(&kind).as_deref(),
            );
            crate::model::Pane::new_with_surface(new_pane_id, new_tab_id, name, surface)
        };

        // Phase 3: workspace pane tree mutate
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);

        // Phase 4: engine mutate (pane borrow 끝)
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();

        Ok(vec![CoreEvent::PaneSplit {
            workspace_index: ws_idx,
            original_pane_id: target_pane_id,
            new_pane_id,
            new_surface_id,
            direction,
        }])
    }

    /// `DomainIntent::SplitSurface` 본문. tab 안에서 surface 추가 (pane tree 변경 X).
    fn apply_split_surface(
        engine: &mut crate::core::CoreState,
        target_surface_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let new_surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        // Phase 1: 새 surface 생성. terminal 은 store 에 직접 insert 후 marker leaf 만,
        // 그 외는 registry.
        let new_surface: Box<dyn crate::model::Surface> = if is_terminal {
            let cols = engine.default_cols;
            let rows = engine.default_rows;
            let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
            let waker = engine.make_waker(new_surface_id);
            let terminal = tasty_terminal::Terminal::new(
                tasty_terminal::TerminalConfig {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    args: &sh.args_ref(),
                    surface_id: new_surface_id,
                    working_dir: cwd.as_deref(),
                    initial_input: None,
                },
                waker,
            )?;
            engine.terminals.insert(new_surface_id, terminal);
            Box::new(crate::model::TerminalSurface { id: new_surface_id })
        } else {
            engine.create_surface_via_registry(
                &kind,
                new_surface_id,
                cwd.as_deref(),
                &surface_params,
            )?
        };

        // Phase 2: tab 안 split
        let (ws_idx, pane_id) = engine
            .find_workspace_index_for_surface(target_surface_id)
            .ok_or_else(|| anyhow::anyhow!("surface {} not found", target_surface_id))?;
        {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;
            pane.split_surface_by_id_with_surface(target_surface_id, direction, new_surface)?;
        }

        // Phase 3: engine mutate (pane borrow 끝)
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();

        Ok(vec![CoreEvent::SurfaceSplit {
            workspace_index: ws_idx,
            pane_id,
            target_surface_id,
            new_surface_id,
        }])
    }

    /// `DomainIntent::ClosePane` 본문. pane_id 로 모든 workspace 순회.
    /// cleanup_targets 수집 → pane tree close → workspace 안 focused_pane 보정
    /// (닫힌 곳의 자연 이동, 원칙 위반 아님). cleanup_surface 는 cascade.
    fn apply_close_pane(engine: &mut crate::core::CoreState, pane_id: u32) -> CoreEvent {
        let ws_idx = match engine.find_workspace_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => {
                return CoreEvent::PaneClosed {
                    pane_id,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = engine.workspaces[ws_idx].pane_layout().find_pane(pane_id) {
            for tab in &pane.tabs {
                crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
            }
        }

        let ws = &mut engine.workspaces[ws_idx];
        let was_focused = ws.focused_pane == pane_id;
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if was_focused && let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            engine.mark_layout_dirty();
        }
        CoreEvent::PaneClosed {
            pane_id,
            closed: removed,
            cleanup_targets: if removed { targets } else { vec![] },
        }
    }

    /// `DomainIntent::ConvertSurface` 본문. tab 안 split leaf 만 교체 / sole
    /// surface tab 전체 교체. 옛 `replace_surface_for_id` + 4 variant 의
    /// surface 생성 로직 흡수.
    fn apply_convert_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        target: crate::core::intent::ConvertSurfaceTarget,
    ) -> CoreEvent {
        use crate::core::intent::ConvertSurfaceTarget;

        let is_terminal = matches!(target, ConvertSurfaceTarget::Terminal { .. });

        // Phase 1: 새 surface 생성 (실패 가능)
        let (new_surface, new_name): (Box<dyn crate::model::Surface>, Option<Option<String>>) =
            match target {
                ConvertSurfaceTarget::Terminal { cwd } => {
                    let cols = engine.default_cols;
                    let rows = engine.default_rows;
                    let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
                    let waker = engine.make_waker(surface_id);
                    let terminal = match tasty_terminal::Terminal::new(
                        tasty_terminal::TerminalConfig {
                            cols,
                            rows,
                            shell: sh.shell_ref(),
                            args: &sh.args_ref(),
                            surface_id,
                            working_dir: cwd.as_deref(),
                            initial_input: None,
                        },
                        waker,
                    ) {
                        Ok(t) => t,
                        Err(_) => {
                            return CoreEvent::SurfaceConverted {
                                surface_id,
                                replaced: false,
                                is_terminal,
                            };
                        }
                    };
                    engine.terminals.insert(surface_id, terminal);
                    let node = crate::model::TerminalSurface { id: surface_id };
                    // Terminal 변환은 explicit_name 클리어 (auto-derived from CWD).
                    (Box::new(node), Some(None))
                }
                ConvertSurfaceTarget::Kind { cwd, kind, params } => {
                    let new_surface = match engine.create_surface_via_registry(
                        &kind,
                        surface_id,
                        cwd.as_deref(),
                        &params,
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("ConvertSurface kind='{}' failed: {}", kind, e);
                            return CoreEvent::SurfaceConverted {
                                surface_id,
                                replaced: false,
                                is_terminal,
                            };
                        }
                    };
                    // markdown 등 file 기반 kind 는 옛 Markdown variant 처럼
                    // file basename 으로 자동 명명. 그 외 kind 는 클리어 — surface
                    // 자체의 display_name 이 사용된다.
                    let auto_name =
                        derive_auto_name(engine.surface_registry.get(&kind).as_deref(), &params);
                    (new_surface, Some(auto_name))
                }
            };

        // Phase 2: location 찾기 (workspace index, pane id, tab index)
        let mut location: Option<(usize, u32, usize)> = None;
        'outer: for (ws_idx, workspace) in engine.workspaces.iter().enumerate() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if tab.contains_surface(surface_id) {
                            location = Some((ws_idx, pid, tab_idx));
                            break 'outer;
                        }
                    }
                }
            }
        }
        let (ws_idx, pane_id, tab_idx) = match location {
            Some(loc) => loc,
            None => {
                return CoreEvent::SurfaceConverted {
                    surface_id,
                    replaced: false,
                    is_terminal,
                };
            }
        };

        // Phase 3: replace
        let replaced = {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
                Some(p) => p,
                None => {
                    return CoreEvent::SurfaceConverted {
                        surface_id,
                        replaced: false,
                        is_terminal,
                    };
                }
            };
            let tab = &mut pane.tabs[tab_idx];
            if tab.is_split() {
                // Tab has split layout — replace just the leaf. Tab name 은 변경 X.
                tab.layout_mut().replace_surface(surface_id, new_surface)
            } else {
                // Tab's sole surface — replace whole tab surface.
                tab.put_surface(new_surface);
                if let Some(name_opt) = new_name {
                    tab.explicit_name = name_opt;
                }
                true
            }
        };

        // Phase 4: engine mutate (pane borrow 끝)
        if replaced {
            engine.mark_layout_dirty();
            if is_terminal {
                engine.send_fast_init(surface_id);
            }
            // 변환으로 focused surface 의 종류/title 이 바뀌었을 수 있고 explicit_name
            // 이 해제(new_name=Some(None))됐을 수도 있으므로 osc_title 를 재투영한다
            // (explicit_name 이 남아있으면 refresh 가 no-op). 새 surface 가 title
            // 미보유(non-terminal 등)면 clear → fallback.
            engine.refresh_tab_osc_title(surface_id);
        }

        CoreEvent::SurfaceConverted {
            surface_id,
            replaced,
            is_terminal,
        }
    }

    /// `DomainIntent::CloseSurface` 본문. cascading close — surface→tab→pane→
    /// workspace 단계까지 자동 cascade. 옛 `close_surface_by_id_inner` 의 4-case
    /// 코드 이동. cleanup_surface / memory purge / active_workspace 보정 /
    /// auto-recreate 는 cascade + caller 책임.
    /// surface→tab→pane→workspace cascade close 디스패처. Step1 판정
    /// (`locate_surface_in_pane`)으로 위치를 잡고 case1..4 헬퍼에 순차 위임한다.
    fn apply_close_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        let loc = match locate_surface_in_pane(engine, surface_id) {
            Some(l) => l,
            None => return surface_close_not_found(surface_id),
        };
        if !loc.surface_is_sole_in_tab && loc.can_close_surface_in_group {
            return Self::close_case_split(engine, &loc, surface_id, save_snapshot)
                .unwrap_or_else(|| surface_close_not_found(surface_id));
        }
        if let Some(ev) = Self::close_case_tab(engine, &loc, surface_id, save_snapshot) {
            return ev;
        }
        if let Some(ev) = Self::close_case_pane(engine, &loc, surface_id) {
            return ev;
        }
        Self::close_case_workspace(engine, &loc, surface_id, save_snapshot)
    }

    /// Case 1: split tab 안 surface 다중 close. Some=닫힘, None=close 실패(→not_found).
    fn close_case_split(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let tab_name_opt = {
                let ws = &engine.workspaces[loc.ws_idx];
                let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
                let tab = &pane.tabs[loc.tab_idx];
                if terminal_surface_in_tab(tab, surface_id).is_some() {
                    Some(tab.display_name().to_string())
                } else {
                    None
                }
            };
            if let Some(tab_name) = tab_name_opt {
                let snapshot = crate::model::closed_item::ClosedSurface::from_surface_id(
                    surface_id,
                    engine.terminals.get(surface_id),
                );
                engine.push_closed_item(crate::model::ClosedItem::Surface {
                    surface: snapshot,
                    tab_name,
                });
            }
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        let tab = &mut pane.tabs[loc.tab_idx];
        let closed = tab.close_surface(surface_id);
        // 닫힌 surface 가 이 탭의 focused 였다면 close_surface 가 focused_surface 를
        // 재배정한다 (배경 탭도 IPC 포커스 독립으로 여기 도달). 새 focused 의 title
        // 로 탭 제목을 재투영해 죽은 surface 의 title 이 남지 않게 한다.
        let new_focused = tab.focused_surface;
        if closed {
            engine.mark_layout_dirty();
            engine.refresh_tab_osc_title(new_focused);
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Surface,
                cleanup_targets: vec![(surface_id, persist_id)],
                closed_tab_ids: vec![],
                closed_pane_ids: vec![],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 2: sole surface tab, pane.tabs.len() > 1 — tab close. None=조건 불충족(fallthrough).
    fn close_case_tab(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                let snapshot_opt = {
                    let mut snap_fn =
                        crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                    let terminals = &engine.terminals;
                    crate::model::closed_item::ClosedTab::from_tab(
                        &pane.tabs[loc.tab_idx],
                        &mut snap_fn,
                        &|id| terminals.get(id),
                    )
                };
                if let Some(snapshot) = snapshot_opt {
                    engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                }
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                crate::state::AppState::collect_close_targets(
                    &pane.tabs[loc.tab_idx],
                    engine,
                    &mut targets,
                );
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            let closed_tab_id = pane.tabs[loc.tab_idx].id;
            pane.tabs.remove(loc.tab_idx);
            if pane.active_tab >= pane.tabs.len() {
                pane.active_tab = pane.tabs.len() - 1;
            }
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Tab,
                cleanup_targets: targets,
                closed_tab_ids: vec![closed_tab_id],
                closed_pane_ids: vec![],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 3: last tab in pane, ws 안 pane >1 — pane close. None=fallthrough.
    fn close_case_pane(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
    ) -> Option<CoreEvent> {
        use crate::core::intent::CascadeLevel;
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
            {
                for tab in &pane.tabs {
                    crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                    closed_tab_ids.push(tab.id);
                }
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.pane_layout_mut().close_pane(loc.pane_id);
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            engine.mark_layout_dirty();
            return Some(CoreEvent::SurfaceClosed {
                surface_id,
                closed: true,
                cascade_level: CascadeLevel::Pane,
                cleanup_targets: targets,
                closed_tab_ids,
                closed_pane_ids: vec![loc.pane_id],
                workspace_id_purged: None,
                workspaces_now_empty: false,
            });
        }
        None
    }

    /// Case 4: last pane in workspace — workspace close. 항상 SurfaceClosed.
    fn close_case_workspace(
        engine: &mut crate::core::CoreState,
        loc: &SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        use crate::core::intent::CascadeLevel;
        if save_snapshot {
            let item = {
                let mut snap_fn =
                    crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[loc.ws_idx];
                let terminals = &engine.terminals;
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
            };
            engine.push_closed_item(item);
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut closed_tab_ids: Vec<u32> = Vec::new();
        let mut closed_pane_ids: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                closed_pane_ids.push(pid);
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                        closed_tab_ids.push(tab.id);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[loc.ws_idx].id;
        engine.workspaces.remove(loc.ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();

        CoreEvent::SurfaceClosed {
            surface_id,
            closed: true,
            cascade_level: CascadeLevel::Workspace,
            cleanup_targets: targets,
            closed_tab_ids,
            closed_pane_ids,
            workspace_id_purged: Some(workspace_id),
            workspaces_now_empty,
        }
    }

    /// `DomainIntent::MoveSurface` 본문 (T9). source(A) 를 살아있는 채로 떼어
    /// target(B) 위치로 replace 한다. B 는 닫힌다(PTY kill). **A 의 Terminal/store/
    /// scrollback 은 절대 만지지 않는다(PTY 보존 — R1).** 모든 위치 탐색은
    /// surface_id 검색식이라 focused_* 같은 사용자 포커스 상태에 의존하지 않는다
    /// (포커스 독립 원칙). 슬롯 비움도 여기서 처리한다.
    fn apply_move_surface(
        engine: &mut crate::core::CoreState,
        source_id: u32,
        target_id: u32,
    ) -> CoreEvent {
        use crate::core::intent::CascadeLevel;

        // 이 intent 가 적용되는 시점에 cut 슬롯은 소비된다 (성공/no-op 무관).
        engine.pending_move_surface = None;

        let noop = || CoreEvent::MoveSurfaceApplied {
            moved: false,
            b_cleanup: None,
            cascade_level: CascadeLevel::Surface,
            closed_tab_ids: vec![],
            closed_pane_ids: vec![],
            workspace_id_purged: None,
            workspaces_now_empty: false,
        };

        // 가드 (명세 항목 6): self-ref / source 무효(이미 닫힘) / target 무효 → no-op.
        if source_id == target_id {
            return noop();
        }
        if engine.find_workspace_index_for_surface(source_id).is_none() {
            return noop();
        }
        if engine.find_workspace_index_for_surface(target_id).is_none() {
            return noop();
        }

        // 1) A 를 트리에서 떼어내 살아있는 Box 획득 (store 불변). sole 이면 A 의 옛
        //    tab/pane/workspace 를 구조적으로 닫고 그 cascade 정보를 함께 받는다.
        let (
            a_box,
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_id_purged,
            workspaces_now_empty,
        ) = match Self::detach_surface_for_move(engine, source_id) {
            Some(v) => v,
            None => return noop(),
        };

        // 2) B 위치 *재검색* (1 단계가 같은-tab 형제 끌어올림 / workspace 제거로
        //    인덱스를 바꿨을 수 있음 — 인덱스 캐시 금지, 매번 id 재검색). 구조적
        //    증명상 B 는 A detach 후에도 항상 살아있다 (B≠A, 공유 구조면 형제 승격).
        let (ws_idx, pane_id) = match engine.find_workspace_index_for_surface(target_id) {
            Some(v) => v,
            None => {
                tracing::error!(
                    source_id,
                    target_id,
                    "move surface: target vanished after detaching source (unreachable)"
                );
                return CoreEvent::MoveSurfaceApplied {
                    moved: false,
                    b_cleanup: None,
                    cascade_level,
                    closed_tab_ids,
                    closed_pane_ids,
                    workspace_id_purged,
                    workspaces_now_empty,
                };
            }
        };

        // 3) B leaf 를 A 로 replace. B 의 옛 id-marker 는 drop 되지만 B 의 Terminal 은
        //    아직 store 에 남아있다 → 4 단계 cleanup 이 PTY kill.
        let b_persist = engine
            .terminals
            .scrollback_persist_id(target_id)
            .map(str::to_string);

        let b_tab_idx = {
            let ws = &engine.workspaces[ws_idx];
            match ws.pane_layout().find_pane(pane_id) {
                Some(pane) => pane.tabs.iter().position(|t| t.contains_surface(target_id)),
                None => None,
            }
        };
        let b_tab_idx = match b_tab_idx {
            Some(i) => i,
            None => {
                tracing::error!(
                    source_id,
                    target_id,
                    "move surface: B tab not found (unreachable)"
                );
                return CoreEvent::MoveSurfaceApplied {
                    moved: false,
                    b_cleanup: None,
                    cascade_level,
                    closed_tab_ids,
                    closed_pane_ids,
                    workspace_id_purged,
                    workspaces_now_empty,
                };
            }
        };

        let replaced = {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .expect("pane re-search must hit (just found above)");
            let tab = &mut pane.tabs[b_tab_idx];
            if tab.is_split() {
                // split 안 leaf 교체 — tab name 불변.
                tab.layout_mut().replace_surface(target_id, a_box)
            } else {
                // B 가 sole 이던 tab — A 가 그 tab 의 단독 surface 가 된다.
                tab.put_surface(a_box);
                true
            }
        };

        if !replaced {
            tracing::error!(
                source_id,
                target_id,
                "move surface: B replace failed (unreachable)"
            );
            return CoreEvent::MoveSurfaceApplied {
                moved: false,
                b_cleanup: None,
                cascade_level,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
            };
        }

        // B(target) 가 이 탭의 focused 였다면 그 자리를 A 가 승계하므로 focused_surface
        // 를 A 로 이어준다 (put_surface 는 sole 케이스에서 이미 A 로 세팅하지만, split
        // replace_surface 는 focused_surface 를 갱신하지 않아 dangling 방지 필요).
        // 그 후 새 focused 의 title 로 탭 제목을 재투영해 죽는 B 의 title 이 남지 않게 한다.
        {
            let ws = &mut engine.workspaces[ws_idx];
            if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                let tab = &mut pane.tabs[b_tab_idx];
                if tab.focused_surface == target_id {
                    tab.focused_surface = source_id;
                }
            }
        }
        engine.mark_layout_dirty();
        engine.refresh_tab_osc_title(source_id);

        CoreEvent::MoveSurfaceApplied {
            moved: true,
            b_cleanup: Some((target_id, b_persist)),
            cascade_level,
            closed_tab_ids,
            closed_pane_ids,
            workspace_id_purged,
            workspaces_now_empty,
        }
    }

    /// `apply_move_surface` 헬퍼 — A(source) 를 트리에서 떼어 살아있는 Box 로 반환.
    /// **A 의 Terminal/store/scrollback 은 절대 만지지 않는다(PTY 보존).** A 가 split
    /// 안 leaf 면 형제를 끌어올리고(`Surface` level), sole-in-tab 이면 그 tab/pane/
    /// workspace 를 `apply_close_surface` Case 2/3/4 와 동형으로 구조적 close 한다 —
    /// 단 **A 의 cleanup_surface/terminals.remove/snapshot 은 일절 없다**(A 는 살아서
    /// 이동). A 못 찾으면 None.
    #[allow(clippy::type_complexity)]
    fn detach_surface_for_move(
        engine: &mut crate::core::CoreState,
        source_id: u32,
    ) -> Option<(
        Box<dyn crate::model::Surface>,
        crate::core::intent::CascadeLevel,
        Vec<u32>,
        Vec<u32>,
        Option<u32>,
        bool,
    )> {
        use crate::core::intent::CascadeLevel;

        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(source_id)?;

        // tab_idx + sole/split 판정.
        let (tab_idx, is_split) = {
            let ws = &engine.workspaces[ws_idx];
            let pane = ws.pane_layout().find_pane(pane_id)?;
            let mut found = None;
            for (i, tab) in pane.tabs.iter().enumerate() {
                if tab.contains_surface(source_id) {
                    found = Some((i, tab.is_split()));
                    break;
                }
            }
            found?
        };

        // Split tab: 형제 끌어올림, A 의 Box 만 반환. 구조적 close 없음.
        if is_split {
            let (a_box, source_tab_focused) = {
                let ws = &mut engine.workspaces[ws_idx];
                let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
                let tab = &mut pane.tabs[tab_idx];
                let layout = tab.take_layout();
                let (new_layout, extracted) = layout.extract_surface(source_id);
                tab.put_layout(new_layout);
                // A 가 이 tab 의 focused 였다면 형제 승격에 맞춰 focused_surface 를
                // 살아있는 surface 로 재배정 (close_surface 와 동일 패턴, dangling 방지).
                if tab.focused_surface == source_id
                    && let Some(first_id) = tab.layout().first_surface_id()
                {
                    tab.focused_surface = first_id;
                }
                let a_box = extracted?; // split 안이면 형제가 있어 항상 Some.
                (a_box, tab.focused_surface)
            };
            engine.mark_layout_dirty();
            // A 가 떠난 source tab 의 제목을 새 focused(형제)의 title 로 재투영해
            // A 의 stale title 이 배경 탭에 남지 않게 한다.
            engine.refresh_tab_osc_title(source_tab_focused);
            return Some((a_box, CascadeLevel::Surface, vec![], vec![], None, false));
        }

        // sole-in-tab: 구조 정보 수집 후 A 의 Box salvage → 구조적 close.
        let (tabs_len, panes_len, tab_id) = {
            let ws = &engine.workspaces[ws_idx];
            let pane = ws.pane_layout().find_pane(pane_id)?;
            (
                pane.tabs.len(),
                ws.pane_layout().all_pane_ids().len(),
                pane.tabs[tab_idx].id,
            )
        };

        // sole leaf 에서 A 의 Box 추출 (tab.layout_opt 는 잠시 None — 동기 경로라 안전).
        let a_box = {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
            let tab = &mut pane.tabs[tab_idx];
            match tab.take_layout() {
                crate::model::SurfaceLayout::Leaf(b) => b,
                other => {
                    // sole 인데 split — 예상 밖. 원복 후 포기.
                    tab.put_layout(other);
                    return None;
                }
            }
        };

        if tabs_len > 1 {
            // Case 2: tab close (pane/workspace 유지).
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
            pane.tabs.remove(tab_idx);
            if pane.active_tab >= pane.tabs.len() {
                pane.active_tab = pane.tabs.len().saturating_sub(1);
            }
            engine.mark_layout_dirty();
            return Some((a_box, CascadeLevel::Tab, vec![tab_id], vec![], None, false));
        }

        if panes_len > 1 {
            // Case 3: pane close (workspace 유지).
            let ws = &mut engine.workspaces[ws_idx];
            ws.pane_layout_mut().close_pane(pane_id);
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            engine.mark_layout_dirty();
            return Some((
                a_box,
                CascadeLevel::Pane,
                vec![tab_id],
                vec![pane_id],
                None,
                false,
            ));
        }

        // Case 4: workspace close. (이동에서 A 가 sole-in-workspace 면 B 는 다른
        //  workspace 에 있으므로 workspaces 가 비지 않는다 — 그래도 일반식으로 계산.)
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();
        Some((
            a_box,
            CascadeLevel::Workspace,
            vec![tab_id],
            vec![pane_id],
            Some(workspace_id),
            workspaces_now_empty,
        ))
    }

    /// `DomainIntent::MoveTab` 본문. pane_id 로 모든 workspace 순회
    /// (focused 의존 없음 — 포커스 독립 원칙).
    fn apply_move_tab(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        from_index: usize,
        to_index: usize,
    ) -> CoreEvent {
        let moved = engine
            .find_pane_by_id_mut(pane_id)
            .map(|p| p.move_tab(from_index, to_index))
            .unwrap_or(false);
        if moved {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabMoved { pane_id, moved }
    }

    /// `DomainIntent::CloseTab` 본문. tab 위치 + cleanup_targets 수집 →
    /// pane.close_tab_by_id → mark_layout_dirty. cleanup_surface (AppState
    /// 데이터) 는 cascade 가 처리한다.
    fn apply_close_tab(engine: &mut crate::core::CoreState, tab_id: u32) -> CoreEvent {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid)
                    && let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id)
                {
                    crate::state::AppState::collect_close_targets(tab, engine, &mut targets);
                    found_pane_id = Some(pid);
                    break;
                }
            }
            if found_pane_id.is_some() {
                break;
            }
        }

        let pane_id = match found_pane_id {
            Some(pid) => pid,
            None => {
                return CoreEvent::TabClosed {
                    tab_id,
                    pane_id: None,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let closed = engine
            .find_pane_by_id_mut(pane_id)
            .map(|p| p.close_tab_by_id(tab_id))
            .unwrap_or(false);
        if closed {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabClosed {
            tab_id,
            pane_id: Some(pane_id),
            closed,
            cleanup_targets: if closed { targets } else { vec![] },
        }
    }

    /// `DomainIntent::CreateTab` 본문. borrow 분리:
    /// 1) settings / waker / surface 미리 추출 (engine 의 *불변* 의존)
    /// 2) scope block 으로 pane mutate (engine 의 가변 borrow 좁힘)
    /// 3) send_fast_init / mark_layout_dirty (pane borrow 끝난 후)
    fn apply_create_tab(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        explicit_name: Option<String>,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);

        let prepared_non_terminal = if !is_terminal {
            let surface = engine.create_surface_via_registry(
                &kind,
                surface_id,
                cwd.as_deref(),
                &surface_params,
            )?;
            let name = crate::state::pane::default_tab_name_for_kind(
                &kind,
                &surface_params,
                engine.surface_registry.get(&kind).as_deref(),
            );
            Some((surface, name))
        } else {
            None
        };

        // Terminal spawn 은 *pane 가변 borrow 시작 전* 에 끝낸다 — store 에 insert
        // 한 뒤 marker 만 pane 에 부착.
        let prepared_terminal = if is_terminal {
            let spawn = crate::model::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                waker,
                working_dir: cwd.as_deref(),
            };
            let terminal = crate::model::Pane::spawn_terminal(surface_id, spawn)?;
            engine.terminals.insert(surface_id, terminal);
            true
        } else {
            false
        };

        {
            let pane = engine
                .find_pane_by_id_mut(pane_id)
                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;
            if is_terminal {
                debug_assert!(prepared_terminal);
                pane.add_terminal_marker_tab_background(tab_id, surface_id, explicit_name);
            } else {
                let (surface, name) = prepared_non_terminal.unwrap();
                pane.add_surface_tab(tab_id, name, explicit_name, surface);
            }
        }

        if is_terminal {
            engine.send_fast_init(surface_id);
        }
        engine.mark_layout_dirty();

        let (tab_count, active_tab) = engine
            .find_pane_by_id(pane_id)
            .map(|p| (p.tabs.len(), p.active_tab))
            .unwrap_or((0, 0));

        Ok(vec![CoreEvent::TabCreated {
            pane_id,
            tab_id,
            surface_id,
            tab_count,
            active_tab,
        }])
    }

    /// `DomainIntent::AdoptTerminal` 본문 — headless PTY 를 실제 Surface 로 승격한다
    /// (`pty.attach_surface`, 18-c). `apply_create_tab` 의 tab/pane 트리 등록은 그대로
    /// 하되 **새 Terminal 을 spawn 하지 않는다**: 이미 `TerminalStore` 에 `pty_id` 키로
    /// 들어있는 headless Terminal 을 새 `surface_id` 로 re-key 하고 `pty_registry` 에서
    /// 제거한다. 같은 Terminal 인스턴스(=같은 PTY 자식 프로세스·scrollback)를 옮기는
    /// 것이라 attach 전 상태가 그대로 보존된다.
    ///
    /// borrow/mutation 순서: 검증 → id 발급 → store re-key(waker 재배선 포함) →
    /// pane marker → registry 제거. pane 미존재 등 실패는 store 를 건드리기 전에
    /// bail 해 orphan 을 만들지 않는다.
    fn apply_adopt_terminal(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        pty_id: u32,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        // 1) 검증 (mutation 전). 대상 headless PTY 가 살아있고 pane 이 존재해야 한다.
        match engine.pty_registry.get(pty_id) {
            None => anyhow::bail!("headless pty {pty_id} not found"),
            Some(entry) if entry.has_exited() => {
                anyhow::bail!("headless pty {pty_id} already exited")
            }
            Some(_) => {}
        }
        if engine.find_pane_by_id(pane_id).is_none() {
            anyhow::bail!("pane {pane_id} not found");
        }

        // 2) 새 id 발급 (apply_create_tab 과 동형).
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();

        // 3) store re-key: headless Terminal 을 pty_id → surface_id 로 옮긴다. 새
        //    surface_id 로 targeted polling 이 이 Terminal 을 그 새 키에서 drain 하도록
        //    waker 를 재배선한다(재배선 없으면 승격된 터미널이 GUI 에서 멈춘 것처럼 보임).
        let Some(terminal) = engine.terminals.remove(pty_id) else {
            anyhow::bail!("headless pty {pty_id} registry/store desync (terminal missing)");
        };
        terminal.rewire_waker(engine.make_waker(surface_id));
        engine.terminals.insert(surface_id, terminal);

        // 4) pane marker: 새 Terminal spawn 없이 트리에만 등록. 에이전트 행동이므로
        //    active_tab 을 바꾸지 않는 background 변형을 쓴다(포커스 독립, 원칙 1·3).
        engine
            .find_pane_by_id_mut(pane_id)
            .expect("pane existence checked above")
            .add_terminal_marker_tab_background(tab_id, surface_id, None);

        // 5) registry 제거: 더 이상 headless 가 아니므로 pty.list 에서 빠지고 이중
        //    등록이 방지된다. 옛 exit-watcher 스레드는 detached 라 자식 reap 을 계속한다.
        engine.pty_registry.remove(pty_id);
        // 옛 pty_id 키의 waker dedup 게이트 제거 — 3)에서 surface_id 로 재배선했으므로
        // 옛 pty_id 게이트는 더 이상 쓰이지 않는다. 미제거 시 승격마다 게이트 누적(누수).
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(pty_id);
        }
        engine.mark_layout_dirty();

        let (tab_count, active_tab) = engine
            .find_pane_by_id(pane_id)
            .map(|p| (p.tabs.len(), p.active_tab))
            .unwrap_or((0, 0));

        Ok(vec![CoreEvent::TabCreated {
            pane_id,
            tab_id,
            surface_id,
            tab_count,
            active_tab,
        }])
    }

    /// `DomainIntent::MoveWorkspace` 본문. workspaces 벡터의 from→to 이동.
    /// active_workspace 보정은 cascade 에서 처리 (Core 는 state 모름).
    fn apply_move_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
        from_index: usize,
        to_index: usize,
    ) -> CoreEvent {
        let len = engine.workspaces.len();
        if from_index == to_index || from_index >= len || to_index >= len {
            return CoreEvent::WorkspaceMoved {
                from_index,
                to_index,
                moved: false,
            };
        }
        let ws = engine.workspaces.remove(from_index);
        engine.workspaces.insert(to_index, ws);
        engine.mark_layout_dirty();
        CoreEvent::WorkspaceMoved {
            from_index,
            to_index,
            moved: true,
        }
    }

    /// `DomainIntent::UpdateWorkspaceMeta` 본문. `workspace_id` 로 찾고 None
    /// 아닌 필드만 갱신. cascade (`cascade_workspace_meta_updated`) 가 host
    /// event 발화.
    fn apply_update_workspace_meta(
        &mut self,
        engine: &mut crate::core::CoreState,
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(index) = engine
            .workspaces
            .iter()
            .position(|ws| ws.id == workspace_id)
        else {
            anyhow::bail!("Workspace id {} not found", workspace_id);
        };

        let ws = &mut engine.workspaces[index];
        if let Some(ref n) = name {
            ws.name = n.clone();
        }
        if let Some(ref s) = subtitle {
            ws.subtitle = s.clone();
        }
        if let Some(ref d) = description {
            ws.description = d.clone();
        }
        engine.mark_layout_dirty();

        Ok(vec![CoreEvent::WorkspaceMetaUpdated {
            workspace_id,
            index,
            name,
            subtitle,
            description,
        }])
    }

    /// `DomainIntent::CreateWorkspace` 본문. engine 에 새 workspace + pane +
    /// tab + surface 를 생성하고 `WorkspaceCreated` event 를 반환한다.
    /// host event 발화 (WorkspaceRenamed) + (User origin 이면) active 전환은
    /// cascade (`cascade_workspace_created`) 에서 처리한다.
    #[allow(clippy::too_many_arguments)] // reason: workspace 생성 도메인 파라미터
    fn apply_create_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
        category: Option<crate::model::WorkspaceCategoryId>,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        Ok(vec![apply_create_workspace_inner(
            engine,
            cwd,
            kind,
            surface_params,
            name,
            subtitle,
            description,
            category,
        )?])
    }

    /// 시스템 내부 invariant restorer — bootstrap / close 후 자동 재생성 /
    /// closed_item precondition 용. `kind="terminal"` + auto name + cwd 미지정.
    /// Intent 큐를 우회하므로 cascade 도중 호출해도 재진입 위험 없음.
    ///
    /// 옛 `AppState::add_workspace` 의 의미를 그대로 유지 — *동작 보존* 위해
    /// host event (WorkspaceCreated/Renamed) 발화하지 않는다. plugin 알림은
    /// 사용자/에이전트 의도 경로 (`DomainIntent::CreateWorkspace`) 만.
    ///
    /// 반환: 새 workspace 의 index (`engine.workspaces.len() - 1`).
    pub(crate) fn create_default_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
    ) -> anyhow::Result<usize> {
        let event = apply_create_workspace_inner(
            engine,
            None,
            "terminal".to_string(),
            serde_json::Value::Null,
            None,
            None,
            None,
            None,
        )?;
        match event {
            CoreEvent::WorkspaceCreated { index, .. } => Ok(index),
            _ => unreachable!("apply_create_workspace_inner 는 WorkspaceCreated 만 반환"),
        }
    }
}

/// `DomainIntent::CreateWorkspace` 의 *순수 engine* 구현. `Core::apply_create_workspace`
/// 와 `Core::create_default_workspace` 양쪽이 공유.
///
/// 반환: `CoreEvent::WorkspaceCreated`. host event (WorkspaceRenamed) +
/// (User origin 이면) active 전환은 호출 측 cascade 책임.
#[allow(clippy::too_many_arguments)] // reason: workspace 생성 도메인 파라미터
pub(crate) fn apply_create_workspace_inner(
    engine: &mut crate::core::CoreState,
    cwd: Option<std::path::PathBuf>,
    kind: String,
    surface_params: serde_json::Value,
    name: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
    category: Option<crate::model::WorkspaceCategoryId>,
) -> anyhow::Result<CoreEvent> {
    if kind == "empty" {
        anyhow::bail!("Cannot create workspace with empty surface kind");
    }

    let ws_id = engine.next_ids.next_workspace();
    let pane_id = engine.next_ids.next_pane();
    let tab_id = engine.next_ids.next_tab();
    let surface_id = engine.next_ids.next_surface();
    let auto_name = name
        .clone()
        .unwrap_or_else(|| format!("Workspace {}", engine.workspaces.len() + 1));
    let is_terminal = kind == "terminal";

    let mut ws = if is_terminal {
        let shell = if engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(engine.settings.general.shell.as_str())
        };
        let shell_args_owned = engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let terminal = crate::model::Pane::spawn_terminal(
            surface_id,
            crate::model::ShellSpawnOpts {
                cols: engine.default_cols,
                rows: engine.default_rows,
                shell,
                shell_args: &shell_args,
                waker: engine.make_waker(surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(surface_id, terminal);
        crate::model::Workspace::new_with_terminal_marker(
            ws_id, auto_name, pane_id, tab_id, surface_id,
        )
    } else {
        let surface = engine.create_surface_via_registry(
            &kind,
            surface_id,
            cwd.as_deref(),
            &surface_params,
        )?;
        let tab_name = crate::state::pane::default_tab_name_for_kind(
            &kind,
            &surface_params,
            engine.surface_registry.get(&kind).as_deref(),
        );
        let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
        crate::model::Workspace::new_with_pane(ws_id, auto_name, pane)
    };

    // 카테고리 소속 지정(존재하는 카테고리만). 없거나 dangling 이면 normal(기본) 유지.
    if let Some(cat_id) = category
        && engine.category_index(cat_id).is_some()
    {
        ws.set_category(cat_id);
    }

    engine.workspaces.push(ws);
    let idx = engine.workspaces.len() - 1;

    let renamed_name = name;
    let renamed_subtitle = subtitle.inspect(|s| {
        engine.workspaces[idx].subtitle = s.clone();
    });
    let renamed_description = description.inspect(|d| {
        engine.workspaces[idx].description = d.clone();
    });

    if is_terminal {
        engine.send_fast_init(surface_id);
    }
    engine.mark_layout_dirty();

    let final_surface_id = {
        let ws = &engine.workspaces[idx];
        let pane_id = ws.focused_pane;
        ws.pane_layout()
            .find_pane(pane_id)
            .and_then(|pane| pane.tabs.get(pane.active_tab))
            .and_then(|tab| tab.focused_surface_id())
    };

    Ok(CoreEvent::WorkspaceCreated {
        id: ws_id,
        index: idx,
        surface_id: final_surface_id,
        renamed_name,
        renamed_subtitle,
        renamed_description,
    })
}

/// `ConvertSurface` 의 Kind 분기에서 사용. kind 가 매니페스트 `name_from_param`
/// (registry `SurfaceKindDef.name_from_param`)을 선언하면 그 params 키 값의 basename 을
/// 자동 명명으로 쓴다(예: markdown="file"). 미선언이면 None — surface 자체의
/// display_name 이 자동 적용된다. 옛 `ConvertSurfaceTarget::Markdown` arm 의 명명 동작을
/// 본체 `kind == "markdown"` 하드코딩 없이 보존한다.
fn derive_auto_name(
    def: Option<&crate::engine::surface_registry::SurfaceKindDef>,
    params: &serde_json::Value,
) -> Option<String> {
    let key = def.and_then(|d| d.name_from_param.as_deref())?;
    let p = params.get(key).and_then(|v| v.as_str())?;
    std::path::Path::new(p)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
}

/// `RestoreClosedItem` 의 helper. pane_id 에 tab attach + active_tab 갱신.
/// *모든* workspace 순회 (포커스 독립).
fn push_tab_to_pane(
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    tab: crate::model::Tab,
) -> bool {
    for ws in engine.workspaces.iter_mut() {
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
            return true;
        }
    }
    false
}

/// Build the OSC 52 read reply (`OSC 52 ; c ; <base64> ST`) for a clipboard
/// query, or `None` when no bytes must be emitted. Returns `None` when `allow`
/// is false (the security gate, off by default) or the clipboard had no text.
/// Isolating this keeps the "off → zero output" invariant unit-testable without
/// constructing a full `Core`.
fn osc52_clipboard_read_reply(allow: bool, clipboard_text: Option<&str>) -> Option<Vec<u8>> {
    if !allow {
        return None;
    }
    let text = clipboard_text?;
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    Some(format!("\x1b]52;c;{encoded}\x07").into_bytes())
}

#[cfg(test)]
mod osc52_clipboard_read_tests {
    use super::osc52_clipboard_read_reply;

    #[test]
    fn off_emits_no_bytes() {
        // Security invariant: a disallowed read query produces zero bytes, even
        // when the clipboard has content.
        assert_eq!(osc52_clipboard_read_reply(false, Some("secret")), None);
        assert_eq!(osc52_clipboard_read_reply(false, None), None);
    }

    #[test]
    fn on_encodes_clipboard_as_osc52() {
        // "hi" → base64 "aGk=", wrapped in `OSC 52 ; c ; <b64> BEL`.
        let reply = osc52_clipboard_read_reply(true, Some("hi")).expect("reply when allowed");
        assert_eq!(reply, b"\x1b]52;c;aGk=\x07".to_vec());
    }

    #[test]
    fn on_with_empty_clipboard_still_replies() {
        // An allowed query with no clipboard text resolves to None (nothing to
        // read), distinct from the gated-off case but also emitting no bytes.
        assert_eq!(osc52_clipboard_read_reply(true, None), None);
    }
}

#[cfg(test)]
mod derive_auto_name_tests {
    use super::derive_auto_name;
    use crate::engine::surface_registry::{SurfaceKindRegistry, register_builtin_kinds};

    #[test]
    fn name_from_param_kind_yields_basename_else_none() {
        let reg = SurfaceKindRegistry::new();
        register_builtin_kinds(&reg);
        // explorer builtin 은 name_from_param="path" 선언 → path basename.
        let explorer = reg.get("explorer").unwrap();
        assert_eq!(
            derive_auto_name(Some(&explorer), &serde_json::json!({"path": "/a/b/proj"})),
            Some("proj".to_string())
        );
        // 선언 키가 없으면 None (자동명명 skip → surface display_name 사용).
        assert_eq!(
            derive_auto_name(Some(&explorer), &serde_json::json!({})),
            None
        );
        // name_from_param 미선언 kind(empty)는 항상 None.
        let empty = reg.get("empty").unwrap();
        assert_eq!(
            derive_auto_name(Some(&empty), &serde_json::json!({"path": "/a/b"})),
            None
        );
        // def 미등록이면 None.
        assert_eq!(
            derive_auto_name(None, &serde_json::json!({"file": "/x.md"})),
            None
        );
    }
}

#[cfg(test)]
mod attach_block_tests {
    use super::*;
    use crate::core::intent::SendPayload;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn attached_surface_blocks_server_send() {
        let mut engine = test_engine();
        // 알려진 id 의 detached mirror terminal 을 직접 등록(기본 워크스페이스
        // 터미널과 무관한 deterministic id).
        let sid = 9999;
        engine
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24));

        // free → 전송 성공.
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));

        // 점유 → 서버 로컬 입력 차단.
        engine.attach.acquire(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: false, .. }));

        // 해제 → 다시 서버 조작 가능.
        engine.attach.release(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }

    #[test]
    fn soft_occupied_surface_allows_server_send() {
        // ADR-0040: soft 점유는 hard 술어(is_hard_occupied)를 세우지 않으므로 서버 로컬
        // 입력이 계속 도달한다(sent: true). hard 만 차단(위 테스트와 대비).
        let mut engine = test_engine();
        let sid = 9998;
        engine
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24));
        engine.occupy_soft(sid, /*parent*/ 1, None).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }
}

#[cfg(test)]
mod move_surface_tests {
    use super::*;
    use crate::model::SplitDirection;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// R1(PTY 보존) 잠금: A 를 B 위치로 이동해도 A 의 Terminal 은 store 에 그대로
    /// 남고(이동=분리+재부착, kill 아님), 이벤트는 B 를 cleanup 대상으로 보고한다.
    /// B 의 store 제거는 cascade(dispatch_domain) 책임이라 apply 단계에선 미발생.
    #[test]
    fn move_preserves_source_terminal_and_reports_b_cleanup() {
        let mut engine = test_engine();
        // 기본 워크스페이스의 단일 surface = A. detached mirror 를 직접 등록해
        // 실제 PTY 스폰 없이 deterministic 하게 store 점유를 만든다.
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .terminals
            .insert(a, tasty_terminal::Terminal::new_detached(80, 24));

        // A 와 같은 tab 에 B 를 split 으로 추가.
        let b = 7777;
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(a, SplitDirection::Horizontal, b)
            .unwrap();
        engine
            .terminals
            .insert(b, tasty_terminal::Terminal::new_detached(80, 24));

        // 사전 조건.
        assert!(engine.terminals.contains(a));
        assert!(engine.terminals.contains(b));
        assert!(engine.find_workspace_index_for_surface(b).is_some());

        let ev = Core::apply_move_surface(&mut engine, a, b);

        // 이벤트: moved=true, B 가 cleanup 대상.
        match ev {
            CoreEvent::MoveSurfaceApplied {
                moved, b_cleanup, ..
            } => {
                assert!(moved, "move must succeed");
                assert_eq!(b_cleanup.map(|(id, _)| id), Some(b));
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // R1: A 의 Terminal 은 store 에 그대로 (PTY 보존).
        assert!(
            engine.terminals.contains(a),
            "source terminal must survive move"
        );
        // A 는 여전히 트리에 존재.
        assert!(engine.find_workspace_index_for_surface(a).is_some());
        // B 의 id-marker 는 replace 로 트리에서 사라짐. 단, B 의 Terminal store
        // 제거는 cascade(dispatch_domain) 책임이라 apply 직후엔 아직 남아있다.
        assert!(engine.find_workspace_index_for_surface(b).is_none());
        assert!(
            engine.terminals.contains(b),
            "apply 단계는 B store 를 건드리지 않는다 (cascade 가 kill)"
        );
        // cut 슬롯 소비.
        assert!(engine.pending_move_surface.is_none());
    }

    /// self-ref(source==target) 는 no-op.
    #[test]
    fn move_self_ref_is_noop() {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.pending_move_surface = Some(a);
        let ev = Core::apply_move_surface(&mut engine, a, a);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: false, .. }
        ));
        // no-op 여도 슬롯은 소비된다.
        assert!(engine.pending_move_surface.is_none());
    }

    /// target 부재(이미 닫힘) 는 no-op, A 는 무사.
    #[test]
    fn move_missing_target_is_noop() {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .terminals
            .insert(a, tasty_terminal::Terminal::new_detached(80, 24));
        engine.pending_move_surface = Some(a);

        let ev = Core::apply_move_surface(&mut engine, a, 999_999);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: false, .. }
        ));
        // A 는 그대로 살아있고 슬롯만 소비.
        assert!(engine.terminals.contains(a));
        assert!(engine.find_workspace_index_for_surface(a).is_some());
        assert!(engine.pending_move_surface.is_none());
    }
}

#[cfg(test)]
mod tab_title_tests {
    //! 탭 제목이 그 탭의 *focused* surface 가 발화한 OSC title 만 반영하는지 검증.
    //! 병렬 surface 의 title 발화가 last-writer-wins 로 제목을 흔드는 flicker 회귀 방지.
    use super::*;
    use crate::model::SplitDirection;
    use tasty_terminal::Terminal;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 기본 워크스페이스 단일 탭에 A(focused)+B 를 split 로 구성. 두 surface 모두
    /// detached terminal 로 store 에 등록. 반환 `(engine, pane_id, a, b)`.
    fn split_tab_engine() -> (CoreState, u32, u32, u32) {
        let mut engine = test_engine();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.terminals.insert(a, Terminal::new_detached(80, 24));
        let b = 7777;
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(a, SplitDirection::Horizontal, b)
            .unwrap();
        engine.terminals.insert(b, Terminal::new_detached(80, 24));
        set_focused(&mut engine, pane_id, a);
        (engine, pane_id, a, b)
    }

    fn set_focused(engine: &mut CoreState, pane_id: u32, sid: u32) {
        engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .tabs[0]
            .focused_surface = sid;
    }

    /// OSC 2 를 feed 해 해당 surface 의 `current_title` 을 세팅한다.
    fn set_title(engine: &mut CoreState, sid: u32, title: &str) {
        engine
            .terminals
            .get_mut(sid)
            .unwrap()
            .feed_bytes(format!("\x1b]2;{title}\x07").as_bytes());
    }

    fn display_name(engine: &CoreState, pane_id: u32) -> String {
        engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs[0]
            .display_name()
    }

    /// 비-focused surface 의 title 발화는 탭 제목을 흔들지 않는다. focused 발화만 반영.
    #[test]
    fn non_focused_surface_title_does_not_change_tab_name() {
        let (mut engine, pane_id, a, b) = split_tab_engine();
        // A 가 focused. B(non-focused)가 title 발화 → 탭 제목 불변.
        let ev = Core::apply_update_tab_name(&mut engine, b, "TITLE-FROM-B".to_string());
        assert!(matches!(
            ev,
            CoreEvent::TabNameUpdated {
                skipped_explicit: false,
                ..
            }
        ));
        assert_ne!(display_name(&engine, pane_id), "TITLE-FROM-B");

        // A(focused)가 발화 → 탭 제목 = TITLE-A.
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id), "TITLE-A");
    }

    /// explicit_name 이 있으면 focused surface 발화도 무시(고정 이름 보존).
    #[test]
    fn explicit_name_survives_focused_title() {
        let (mut engine, pane_id, a, _b) = split_tab_engine();
        engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .tabs[0]
            .explicit_name = Some("FIXED".to_string());
        let ev = Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert!(matches!(
            ev,
            CoreEvent::TabNameUpdated {
                skipped_explicit: true,
                ..
            }
        ));
        assert_eq!(display_name(&engine, pane_id), "FIXED");
    }

    /// 포커스가 B 로 이동하면 재투영으로 B 의 최신 title(unfocused 시절 발화분)이 반영.
    #[test]
    fn refresh_projects_new_focused_surface_title() {
        let (mut engine, pane_id, a, b) = split_tab_engine();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id), "TITLE-A");

        // 포커스를 B 로 전환 후 재투영 → B title.
        set_focused(&mut engine, pane_id, b);
        engine.refresh_tab_osc_title(b);
        assert_eq!(display_name(&engine, pane_id), "TITLE-B");
    }

    /// 새 focused surface 가 title 미보유면 osc_title clear → fallback 동작.
    #[test]
    fn refresh_clears_when_focused_has_no_title() {
        let (mut engine, pane_id, a, b) = split_tab_engine();
        set_title(&mut engine, a, "TITLE-A");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id), "TITLE-A");

        // B 는 title 없음 → 포커스 B 로 전환 + 재투영 → osc_title clear → fallback.
        set_focused(&mut engine, pane_id, b);
        engine.refresh_tab_osc_title(b);
        assert_ne!(display_name(&engine, pane_id), "TITLE-A");
    }

    /// focused surface 를 close 하면 생존 surface 로 focused 재배정 + 제목 재투영.
    #[test]
    fn closing_focused_surface_reprojects_to_survivor() {
        let (mut engine, pane_id, a, b) = split_tab_engine();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id), "TITLE-A");

        // focused A 를 close → 생존 B 로 focused 재배정 + 재투영 → B title.
        let ev = Core::apply_close_surface(&mut engine, a, false);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));
        assert_eq!(display_name(&engine, pane_id), "TITLE-B");
    }

    /// surface move 로 target tab 의 focused 가 A 로 승계되면 제목이 A 로 재투영.
    #[test]
    fn moving_surface_reprojects_target_tab_title() {
        let (mut engine, pane_id, a, b) = split_tab_engine();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        // focused=B 로 두고 B title 투영 (move 전 stale 상황 유도).
        set_focused(&mut engine, pane_id, b);
        engine.refresh_tab_osc_title(b);
        assert_eq!(display_name(&engine, pane_id), "TITLE-B");

        // A 를 B 위치로 move → 탭은 A 단독, 제목이 A 로 재투영 (B 의 stale title 제거).
        engine.pending_move_surface = Some(a);
        let ev = Core::apply_move_surface(&mut engine, a, b);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: true, .. }
        ));
        assert_eq!(display_name(&engine, pane_id), "TITLE-A");
    }
}

#[cfg(test)]
mod close_surface_cascade_tests {
    //! `apply_close_surface` (C2) 의 반환 `CoreEvent::SurfaceClosed` 필드
    //! characterization. Case2(tab)/Case3(pane)/Case4(workspace) cascade 의
    //! `cleanup_targets`·`closed_tab_ids`·`closed_pane_ids`·`workspace_id_purged`·
    //! `workspaces_now_empty`·`cascade_level` 을 고정한다. 필드 하나라도 누락되면
    //! caller `cascade_surface_closed` 가 plugin lifecycle 큐·host TabClosed·
    //! memory purge 를 건너뛰어 런타임에서만 드러나는 leak 이 되므로, case별 헬퍼
    //! 추출 리팩터의 안전망이다. save_snapshot=false 로 호출해 undo 스택/스냅샷
    //! 경로는 배제하고 순수 cascade 반환값만 고정한다.
    use super::*;
    use crate::core::intent::CascadeLevel;
    use tasty_terminal::Terminal;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn insert_detached(engine: &mut CoreState, sid: u32) {
        engine.terminals.insert(sid, Terminal::new_detached(80, 24));
    }

    /// Case 2: sole-surface tab & pane 에 tab >1 → tab close.
    #[test]
    fn case2_tab_close_returns_tab_level_fields() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid0).unwrap();
        // 두 번째 탭(sole surface) 추가.
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .add_terminal_marker_tab(tab1_id, sid1);

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Tab);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_id_purged, None);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(
            engine.workspaces[ws_idx]
                .pane_layout()
                .find_pane(pane_id)
                .unwrap()
                .tabs
                .len(),
            1
        );
    }

    /// Case 3: last tab in pane & ws 에 pane >1 → pane close.
    #[test]
    fn case3_pane_close_returns_pane_level_fields() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        let (ws_idx, pane0) = engine.find_workspace_index_for_surface(sid0).unwrap();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        let new_pane = crate::model::Pane::new_with_terminal_marker(pane1_id, tab1_id, sid1);
        let leftover = engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(pane0, crate::model::SplitDirection::Horizontal, new_pane);
        assert!(leftover.is_none(), "split 성공해야 함");

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Pane);
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert_eq!(workspace_id_purged, None);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(
            engine.workspaces[ws_idx].pane_layout().all_pane_ids().len(),
            1
        );
    }

    /// Case 4: last pane in workspace, 다른 workspace 생존 → workspace close.
    #[test]
    fn case4_workspace_close_returns_workspace_level_fields() {
        let mut engine = test_engine();
        let ws1_id = engine.next_ids.next_workspace();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        insert_detached(&mut engine, sid1);
        let ws1 = crate::model::Workspace::new_with_terminal_marker(
            ws1_id,
            "ws1".to_string(),
            pane1_id,
            tab1_id,
            sid1,
        );
        engine.workspaces.push(ws1);
        assert_eq!(engine.workspaces.len(), 2);

        let ev = Core::apply_close_surface(&mut engine, sid1, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_id_purged, Some(ws1_id));
                assert_eq!(closed_pane_ids, vec![pane1_id]);
                assert_eq!(closed_tab_ids, vec![tab1_id]);
                assert_eq!(cleanup_targets, vec![(sid1, None)]);
                assert!(!workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert_eq!(engine.workspaces.len(), 1);
    }

    /// Case 4 변형: 마지막 workspace 를 닫으면 `workspaces_now_empty==true`.
    #[test]
    fn case4_last_workspace_reports_now_empty() {
        let mut engine = test_engine();
        let sid0 = engine.workspaces[0].all_surface_ids()[0];
        insert_detached(&mut engine, sid0);
        let ws0_id = engine.workspaces[0].id;

        let ev = Core::apply_close_surface(&mut engine, sid0, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                workspace_id_purged,
                workspaces_now_empty,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Workspace);
                assert_eq!(workspace_id_purged, Some(ws0_id));
                assert!(workspaces_now_empty);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
        assert!(engine.workspaces.is_empty());
    }

    /// Case 1 보강: split tab 다중 close 의 반환 필드(기존 title-재투영 테스트는 미검증).
    #[test]
    fn case1_split_close_returns_single_cleanup_target() {
        let mut engine = test_engine();
        let sid_a = engine.workspaces[0].all_surface_ids()[0];
        insert_detached(&mut engine, sid_a);
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(sid_a).unwrap();
        let sid_b = engine.next_ids.next_surface();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(sid_a, crate::model::SplitDirection::Horizontal, sid_b)
            .unwrap();
        insert_detached(&mut engine, sid_b);

        let ev = Core::apply_close_surface(&mut engine, sid_a, false);
        match ev {
            CoreEvent::SurfaceClosed {
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_id_purged,
                ..
            } => {
                assert!(closed);
                assert_eq!(cascade_level, CascadeLevel::Surface);
                assert_eq!(cleanup_targets, vec![(sid_a, None)]);
                assert!(closed_tab_ids.is_empty());
                assert!(closed_pane_ids.is_empty());
                assert_eq!(workspace_id_purged, None);
            }
            other => panic!("expected SurfaceClosed, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod mirror_structural_guard_tests {
    //! mirror(원격 attach client) 워크스페이스 누출 차단 (1단계). mirror 워크스페이스의
    //! surface/pane 을 target 으로 한 구조 `DomainIntent` 를 `Core::apply` 로 디스패치하면
    //! 로컬 실행이 거부되고([`MirrorStructuralBlocked`]) **새 로컬 터미널이 insert 되지
    //! 않아야** 한다. 비-mirror 워크스페이스는 그대로 통과(회귀 방지).
    use super::*;
    use crate::core::intent::DomainIntent;
    use crate::model::SplitDirection;
    use tasty_terminal::Terminal;

    /// 테스트용 `Core` — 모든 port 를 mock/in-memory 로 주입. `apply` 의 mirror 가드는
    /// 어떤 port 도 건드리기 전에 반환하므로 실제 PTY/디스크 접근이 없다.
    fn build_test_core() -> (Core, CoreState) {
        use std::sync::{Arc, Mutex};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::core::builder::CoreBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let engine = CoreState::new(80, 24, waker).expect("engine");

        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn tasty_memory::MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn tasty_themes::ThemeStorage> = Arc::new(tasty_themes::ThemeStore::new());

        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner::default()))
            .with_home(Arc::new(TmpHome::new(
                tempfile::tempdir().expect("tmp").keep(),
            )))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core");
        (core, engine)
    }

    /// 기본 워크스페이스 0 의 단일 surface `a` 에 detached 터미널을 붙이고 `(surface, pane)`
    /// 를 반환. `mirror` 는 호출자가 세팅.
    fn seed(engine: &mut CoreState) -> (u32, u32) {
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.terminals.insert(a, Terminal::new_detached(80, 24));
        let (_ws, pane) = engine.find_workspace_index_for_surface(a).unwrap();
        (a, pane)
    }

    fn is_blocked(err: &anyhow::Error) -> bool {
        err.downcast_ref::<MirrorStructuralBlocked>().is_some()
    }

    /// mirror 워크스페이스에서 SplitSurface/SplitPane/CreateTab 디스패치 시 거부 +
    /// 새 로컬 터미널 insert 없음. (수정 전이라면 로컬 PTY 가 spawn 돼 count 가 늘어난다.)
    #[test]
    fn mirror_split_and_newtab_are_blocked_without_spawning() {
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let before = engine.terminals.iter().count();

        for intent in [
            DomainIntent::SplitSurface {
                target_surface_id: a,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
            DomainIntent::SplitPane {
                target_pane_id: pane,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
            DomainIntent::CreateTab {
                pane_id: pane,
                cwd: None,
                kind: "terminal".to_string(),
                name: None,
                surface_params: serde_json::json!({}),
            },
        ] {
            let err = core
                .apply(&mut engine, intent)
                .expect_err("must be blocked");
            assert!(
                is_blocked(&err),
                "expected MirrorStructuralBlocked, got: {err}"
            );
            assert_eq!(
                engine.terminals.iter().count(),
                before,
                "mirror 워크스페이스에서 새 로컬 터미널이 insert 되면 안 된다"
            );
        }
    }

    /// 비-mirror 워크스페이스는 가드에 걸리지 않는다(회귀 방지). SplitSurface 가
    /// 통과해 새 터미널이 실제로 insert 된다.
    #[test]
    fn non_mirror_split_passes_and_spawns() {
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        assert!(!engine.workspaces[0].mirror);
        let before = engine.terminals.iter().count();

        core.apply(
            &mut engine,
            DomainIntent::SplitSurface {
                target_surface_id: a,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
        )
        .expect("non-mirror split must succeed");
        assert_eq!(
            engine.terminals.iter().count(),
            before + 1,
            "비-mirror split 은 로컬 터미널을 1개 늘려야 한다(회귀)"
        );
    }

    /// 18-c e2e: headless PTY spawn 흉내 → `AdoptTerminal` 승격 → (1) 같은 Terminal
    /// 인스턴스가 pty_id→surface_id 로 re-key 되어 상태 보존, (2) registry 에서 제거,
    /// (3) pane tab 목록에 등장, (4) `TabCreated` cascade 이벤트 발행.
    #[test]
    fn adopt_terminal_promotes_headless_pty_preserving_state() {
        use crate::core::pty_registry::PtySpawnSpec;

        let (mut core, mut engine) = build_test_core();
        let (_a, pane) = seed(&mut engine);

        // pty.spawn 흉내: registry 등록 + 같은 pty_id 로 real Terminal 삽입.
        let pty_id = engine
            .pty_registry
            .register(
                PtySpawnSpec {
                    owner_agent_id: "agent-x".into(),
                    cwd: None,
                    command: vec![],
                },
                std::time::Instant::now(),
            )
            .expect("register headless pty");
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(pty_id);
        let terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                surface_id: pty_id,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("spawn headless terminal");
        engine.terminals.insert(pty_id, terminal);

        // 승격 전에 상태를 만들어 둔다 — 같은 프로세스라면 승격 후에도 화면에 남는다.
        engine
            .find_terminal_by_id_mut(pty_id)
            .expect("headless terminal")
            .send_bytes(b"echo ADOPT_MARKER_123\n");
        let mut seen = false;
        for _ in 0..500 {
            engine.process_surface(pty_id);
            if engine
                .find_terminal_by_id(pty_id)
                .map(|t| t.screen_text(true))
                .unwrap_or_default()
                .contains("ADOPT_MARKER_123")
            {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(seen, "marker should appear before adoption");

        // 승격.
        let events = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id,
                },
            )
            .expect("adopt must succeed");
        let surface_id = match events.into_iter().next() {
            Some(CoreEvent::TabCreated {
                surface_id,
                pane_id: p,
                ..
            }) => {
                assert_eq!(p, pane, "TabCreated pane_id");
                surface_id
            }
            other => panic!("expected TabCreated cascade event, got {other:?}"),
        };

        // (1) re-key: pty_id 는 사라지고 surface_id 로 옮겨졌으며 상태가 보존됐다.
        assert!(
            engine.find_terminal_by_id(pty_id).is_none(),
            "old pty_id key removed from store"
        );
        let screen = engine
            .find_terminal_by_id(surface_id)
            .expect("terminal now at surface_id")
            .screen_text(true);
        assert!(
            screen.contains("ADOPT_MARKER_123"),
            "state preserved across promotion (same process): {screen:?}"
        );

        // (2) registry 에서 제거 — pty.list 에서 빠지고 이중 등록 방지.
        assert!(
            !engine.pty_registry.contains(pty_id),
            "promoted pty must leave the headless registry"
        );

        // (3) pane tab 목록에 새 surface 등장.
        let pane_ref = engine.find_pane_by_id(pane).expect("pane");
        assert!(
            pane_ref
                .tabs
                .iter()
                .any(|t| t.all_surface_ids().contains(&surface_id)),
            "promoted surface must appear in the pane's tabs"
        );

        // 정리: 승격된 surface 의 Terminal 제거(프로세스 종료).
        engine.terminals.remove(surface_id);
    }

    /// 회귀(waker dedup 게이트 누수): `AdoptTerminal` 승격은 Terminal 을
    /// pty_id→surface_id 로 re-key 하며 새 surface_id 게이트를 배선하므로, 옛 pty_id
    /// 게이트를 `forget_surface` 로 정리해야 한다(미정리 시 승격마다 누적).
    #[test]
    fn adopt_terminal_forgets_old_pty_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        use crate::core::pty_registry::PtySpawnSpec;

        let (mut core, mut engine) = build_test_core();
        let factory = RecordingWakerFactory::new();
        let shared: crate::waker::SharedWakerFactory = factory.clone();
        engine.waker_factory = Some(shared);
        let (_a, pane) = seed(&mut engine);

        // pty.spawn 흉내: registry 등록 + make_waker(pty_id) 로 pty_id 게이트 생성.
        let pty_id = engine
            .pty_registry
            .register(
                PtySpawnSpec {
                    owner_agent_id: "agent-x".into(),
                    cwd: None,
                    command: vec![],
                },
                std::time::Instant::now(),
            )
            .expect("register headless pty");
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(pty_id);
        let terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                surface_id: pty_id,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("spawn headless terminal");
        engine.terminals.insert(pty_id, terminal);
        assert!(
            factory.made().contains(&pty_id),
            "spawn 흉내는 pty_id 게이트를 만든다"
        );

        // 승격.
        let events = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id,
                },
            )
            .expect("adopt must succeed");
        let surface_id = match events.into_iter().next() {
            Some(CoreEvent::TabCreated { surface_id, .. }) => surface_id,
            other => panic!("expected TabCreated, got {other:?}"),
        };

        // 옛 pty_id 게이트는 정리, 새 surface_id 게이트(재배선된 활성 게이트)는 보존.
        assert!(
            factory.forgotten().contains(&pty_id),
            "adopt 는 옛 pty_id 의 waker 게이트를 정리해야 한다"
        );
        assert!(
            !factory.forgotten().contains(&surface_id),
            "재배선된 새 surface_id 게이트는 정리 대상이 아니다"
        );

        // 정리: 승격된 surface 의 Terminal 제거.
        engine.terminals.remove(surface_id);
    }

    /// 18-c: 존재하지 않는 pty_id 로 승격 시도는 에러 — store/트리 무변경.
    #[test]
    fn adopt_unknown_pty_errors() {
        let (mut core, mut engine) = build_test_core();
        let (_a, pane) = seed(&mut engine);
        let bogus = crate::core::pty_registry::PTY_ID_BASE + 4242;
        let before = engine.terminals.iter().count();
        let err = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id: bogus,
                },
            )
            .expect_err("unknown pty must error");
        assert!(err.to_string().contains("not found"), "err: {err}");
        assert_eq!(
            engine.terminals.iter().count(),
            before,
            "실패한 승격은 store 를 건드리지 않아야 한다"
        );
    }

    /// 순수 판별 헬퍼: 모든 구조 variant 가 mirror 워크스페이스 대상일 때 Some,
    /// mirror 플래그가 없으면 None. (구조와 무관한 intent 는 항상 None.)
    #[test]
    fn helper_flags_structural_targets_only_when_mirror() {
        let (_core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        // 두 번째 탭을 추가해 CloseTab/tab 대상 확보.
        let tab_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        engine
            .terminals
            .insert(sid1, Terminal::new_detached(80, 24));
        engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane)
            .unwrap()
            .add_terminal_marker_tab(tab_id, sid1);

        let structural = |a: u32, pane: u32, tab_id: u32| {
            vec![
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
                DomainIntent::SplitPane {
                    target_pane_id: pane,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
                DomainIntent::CreateTab {
                    pane_id: pane,
                    cwd: None,
                    kind: "terminal".to_string(),
                    name: None,
                    surface_params: serde_json::json!({}),
                },
                DomainIntent::CloseSurface {
                    surface_id: a,
                    save_snapshot: false,
                },
                DomainIntent::ClosePane { pane_id: pane },
                DomainIntent::CloseTab { tab_id },
                DomainIntent::MoveTab {
                    pane_id: pane,
                    from_index: 0,
                    to_index: 1,
                },
            ]
        };

        // 비-mirror: 전부 None.
        for intent in structural(a, pane, tab_id) {
            assert_eq!(
                engine.mirror_workspace_index_for_structural(&intent),
                None,
                "비-mirror 는 통과해야 한다: {intent:?}"
            );
        }
        // mirror: 전부 Some(0).
        engine.workspaces[0].mirror = true;
        for intent in structural(a, pane, tab_id) {
            assert_eq!(
                engine.mirror_workspace_index_for_structural(&intent),
                Some(0),
                "mirror 는 차단 대상이어야 한다: {intent:?}"
            );
        }
        // 구조와 무관한 intent 는 mirror 여도 None.
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&DomainIntent::SetTerminalMark {
                surface_id: a
            }),
            None,
        );
    }

    /// 2단계 client 측: mirror split 은 로컬 실행이 차단되면서 forward 큐에 op 를 쌓는다.
    /// op 의 anchor 는 아직 **로컬** surface id(App drain 이 원격으로 치환), forwarded=true.
    #[test]
    fn mirror_split_enqueues_forward_with_local_anchor() {
        use crate::ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        assert!(engine.pending_structural_forward.is_empty());

        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("mirror split must be blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(blocked.forwarded, "forward 가능 op 는 forwarded=true");
        assert_eq!(engine.pending_structural_forward.len(), 1);
        let queued = &engine.pending_structural_forward[0];
        assert!(
            !queued.user_triggered,
            "Core::apply 는 origin 을 모르므로 기본 user_triggered=false(08)"
        );
        match &queued.op {
            StructuralOp::SplitSurface { surface_id, .. } => {
                assert_eq!(*surface_id, a, "anchor 는 로컬 surface a");
            }
            other => panic!("expected SplitSurface, got {other:?}"),
        }
    }

    /// SplitPane/NewTab 는 pane 의 대표 surface(활성 탭 focused)를 anchor 로 큐잉한다.
    #[test]
    fn mirror_split_pane_anchors_on_pane_surface() {
        use crate::ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        core.apply(
            &mut engine,
            DomainIntent::SplitPane {
                target_pane_id: pane,
                direction: SplitDirection::Vertical,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
        )
        .expect_err("blocked");
        match &engine.pending_structural_forward[0].op {
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            } => assert_eq!(*anchor_surface_id, a, "pane anchor = 활성 탭 surface a"),
            other => panic!("expected SplitPane, got {other:?}"),
        }
    }

    /// convert 는 재사용할 원격 IPC 핸들러가 없어 forward 대상이 아니다 →
    /// forwarded=false(기존 차단 toast 유지), 큐는 비어있다.
    #[test]
    fn mirror_convert_is_blocked_without_forward() {
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::ConvertSurface {
                    surface_id: a,
                    target: crate::core::intent::ConvertSurfaceTarget::Terminal { cwd: None },
                },
            )
            .expect_err("blocked");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(!blocked.forwarded, "convert 는 forward 대상 아님");
        assert!(engine.pending_structural_forward.is_empty());
    }

    /// 08 — `mark_last_forward_user_triggered` 는 `forwarded=true` + user origin 일
    /// 때만 마지막 pending forward 를 `user_triggered=true` 로 뒤집는다.
    #[test]
    fn mark_last_forward_user_triggered_flips_on_user_origin() {
        use crate::intent::{IntentOrigin, UserSource};

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("blocked");
        assert!(!engine.pending_structural_forward[0].user_triggered);

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::User {
                source: UserSource::Shortcut("split_surface_horizontal"),
            },
        );
        assert!(
            engine.pending_structural_forward[0].user_triggered,
            "user origin + forwarded=true 는 뒤집혀야 한다"
        );
    }

    /// 08 — agent/IPC origin 이면 forwarded=true 여도 그대로 false 로 남는다(기존 동작
    /// 유지, IPC 경로는 focus 를 옮기지 않아야 하므로).
    #[test]
    fn mark_last_forward_user_triggered_stays_false_on_agent_origin() {
        use crate::intent::IntentOrigin;

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("blocked");

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::Agent {
                source: crate::intent::AgentSource::Ipc,
            },
        );
        assert!(
            !engine.pending_structural_forward[0].user_triggered,
            "agent origin 은 뒤집히면 안 된다"
        );
    }

    /// 08 — `forwarded=false`(convert 등 forward 불가 op)면 origin 이 user 여도
    /// 아무것도 건드리지 않는다(애초에 큐가 비어 있으므로 no-op).
    #[test]
    fn mark_last_forward_user_triggered_noop_when_not_forwarded() {
        use crate::intent::{IntentOrigin, UserSource};

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::ConvertSurface {
                    surface_id: a,
                    target: crate::core::intent::ConvertSurfaceTarget::Terminal { cwd: None },
                },
            )
            .expect_err("blocked");

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::User {
                source: UserSource::Shortcut("x"),
            },
        );
        assert!(engine.pending_structural_forward.is_empty());
    }
}
