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
pub(crate) mod command_index;
pub(crate) mod file;
pub(crate) mod fs_list;
pub(crate) mod hook_event_registry;
pub(crate) mod intent;
pub(crate) mod ipc_facade;
pub(crate) mod layout_persistence;
pub(crate) mod mesh_mirror;
pub(crate) mod output_observer;
pub(crate) mod pty_registry;
pub(crate) mod restore_rebuild;
pub(crate) mod session;
pub(crate) mod state;
pub(crate) mod surface_registry;
pub(crate) mod terminal_store;

pub(crate) mod app_surface;
#[cfg(debug_assertions)]
pub(crate) mod app_surface_debug;
pub(crate) mod impl_attach;
pub(crate) mod impl_clipboard;
pub(crate) mod impl_close;
pub(crate) mod impl_convert;
pub(crate) mod impl_mirror;
pub(crate) mod impl_move;
pub(crate) mod impl_pty;
pub(crate) mod impl_split;
pub(crate) mod impl_tab;
pub(crate) mod impl_workspace;
pub(crate) mod request_target;

pub(crate) use state::{
    AttachMeshContextForward, AttentionKind, CoreState, GuiAttachUserReq, PendingImageUpload,
};

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

pub(crate) use impl_close::{SurfaceCloseLocation, locate_surface_in_pane};
pub(crate) use impl_mirror::{
    MirrorStructuralBlocked, PendingStructuralForward, mark_last_forward_user_triggered,
};
pub(crate) use impl_workspace::{WorkspaceCreationParams, apply_create_workspace_inner};

/// (04) 파일 피커 원격 `list_dir_request` id 시퀀스 — 프로세스 내 유일성만 필요
/// (capture 의 `upload_id` 시퀀스와 동일 근거).
static NEXT_LIST_DIR_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// 다음 list_dir_request id 발급. 파일 피커 popup wrapper
/// (`adapters::ui::popup::file_picker`)가 원격 조회를 트리거할 때 호출.
pub(crate) fn next_list_dir_request_id() -> u64 {
    NEXT_LIST_DIR_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// git-viewer(원격) `git_query_request` id 시퀀스 — `next_list_dir_request_id` 와
/// 동일 근거(프로세스 내 유일성만 필요).
static NEXT_GIT_QUERY_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// 다음 git_query_request id 발급. `git_viewer.query` IPC 핸들러
/// (`adapters::ipc::handler::git_viewer`)가 원격 조회를 트리거할 때 호출.
pub(crate) fn next_git_query_request_id() -> u64 {
    NEXT_GIT_QUERY_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// `file_picker.trigger` IPC(ADR-0058) 요청 id 시퀀스 — `next_list_dir_request_id`
/// 와 동일 근거(프로세스 내 유일성만 필요). **주의**: 이 id 는 `FpLoadState::Loading`
/// 의 (popup 내부 원격 디렉토리 나열 요청 상관관계) `request_id` 와 완전히 별개의
/// 네임스페이스다 — 둘 다 필드명이 `request_id` 라 혼동하기 쉽다. 이 id 는
/// plugin↔host `file_picker.trigger`/`"file_picker.result"` 왕복 전체를 상관관계
/// 짓고, 저건 popup 내부의 개별 `list_dir` 왕복(디렉토리 이동마다 새로 발급)만
/// 상관관계 짓는다 — 하나의 트리거 생명주기 동안 후자는 여러 번 재발급될 수 있다.
static NEXT_FILE_PICKER_TRIGGER_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// 다음 `file_picker.trigger` request id 발급. `file_picker.trigger` IPC 핸들러
/// (`adapters::ipc::handler::file_picker`)가 plugin 요청을 접수할 때 호출.
pub(crate) fn next_file_picker_trigger_request_id() -> u64 {
    NEXT_FILE_PICKER_TRIGGER_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// memory store 락의 poison 복구가 쓰는 공용 보고 좌표.
///
/// store 는 프로세스에 하나(App 이 소유, `Arc` clone 으로 여러 모듈이 나눠 갖는다)라
/// 첫-1 회 플래그도 하나다. 여러 모듈이 이 락을 잡되 복구는 전부
/// `poison::recover_mutex` 를 거쳐 이 좌표로 모인다 — 조용한 복구를 첫 1 회만 보고한다.
pub(crate) const MEMORY_WHAT: &str = "memory store";
pub(crate) static MEMORY_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// mirror 워크스페이스 원격 디렉토리 목록 forward 큐(`CoreState::pending_list_dir_forward`)
/// 의 원소. popup wrapper/`ExplorerViewStore` outbox 가 (mirror 판별 후) push, App 이
/// `about_to_wait` 에서 drain 해 세션의 attach 채널로 `list_dir_request` 를 전송한다 —
/// 구조 op forward(`PendingStructuralForward`)/resize forward(`pending_resize_forward`)와
/// 동형의 "popup/domain 이 큐에 push, App 레이어가 drain 해 실제 소켓 IO" 패턴.
#[derive(Debug, Clone)]
pub(crate) struct PendingListDirForward {
    pub(crate) local_ws_id: u32,
    pub(crate) request_id: u64,
    pub(crate) dir: String,
    /// (ADR-0059) 이 요청의 **소비자** — `None` = File Picker(기존 단일
    /// `FpLoadState` 매칭), `Some(surface_id)` = explorer(그 surface 의 `ExplorerView`
    /// 가 경로별 pending 상태로 자체 추적). `MirrorEvent::ListDirResult` 도착 시 App
    /// 레이어가 이 태그로 라우팅을 분기한다 — host 범용 "request_id → consumer"
    /// 레지스트리는 만들지 않는다(ADR-0059 Decision 4).
    pub(crate) consumer: Option<u32>,
}

/// git-viewer(원격) git 조회 forward 큐(`CoreState::pending_git_query_forward`)의
/// 원소. `git_viewer.query` IPC 핸들러(plugin 이 host.call 로 트리거)가 push, App 이
/// `about_to_wait` 에서 drain 해 mirror 세션의 attach 채널로 `git_query_request` 를
/// 전송한다 — `PendingListDirForward` 와 동형.
#[derive(Debug, Clone)]
pub(crate) struct PendingGitQueryForward {
    /// popup 이 attach 된 **로컬** mirror surface id — 세션 조회(local→remote 치환)의
    /// 앵커. `list_dir` 와 달리 cwd 를 클라이언트가 미리 계산해 보내지 않고, 서버가
    /// 이 surface 의 원격 대응(`Terminal::get_cwd`)으로 직접 resolve 한다(OSC 7
    /// mirror 재생 의존 제거).
    pub(crate) local_surface_id: u32,
    pub(crate) request_id: u64,
    pub(crate) kind: crate::adapters::production::stream_hub::GitQueryKind,
    /// worktree 전환/새로고침 — 이전 응답이 돌려준 opaque 서버 경로 echo.
    pub(crate) worktree_path: Option<String>,
    /// `kind = Diff` 전용.
    pub(crate) diff_path: Option<String>,
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

    /// hook_id → 대기 중인 agent task 매핑. `HookFired` 소비부가 매 발화마다
    /// 조회한다 — [`crate::core::agent::hook_wait`] 참조.
    pub(crate) hook_task_waits: Arc<crate::core::agent::hook_wait::HookTaskWaits>,
}

impl Core {
    /// `Clock` port 경유 현재 시각(monotonic). outbound port 실제 소비 경로 최소
    /// 1곳 확보(`pty.spawn`, `handler/pty.rs`).
    pub(crate) fn now_instant(&self) -> std::time::Instant {
        self.clock.now_instant()
    }

    /// `Clock` port 경유 현재 Unix ms — 관측 로그(audit/telemetry)의 시각 축이
    /// monotonic 이 아니라 wall-clock 이라 별도 접근자가 필요하다
    /// (`log_retention::maybe_prune` 의 주기 게이트 판정).
    pub(crate) fn now_unix_millis(&self) -> i64 {
        self.clock.now_unix_millis()
    }

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

    /// surface hook 등록. 반환: 새 hook id.
    ///
    /// `binding` 은 공유 훅 핸들러 레지스트리 참조([`HookBinding::Handler`]) 또는
    /// 하위호환 인라인 셸([`HookBinding::InlineShell`]). `OutputMatch` 훅은 PTY
    /// emit 게이트(`sync_output_event_gates`)를 여기서 즉시(eager) 동기화한다 —
    /// `observer_register`/`observer_unregister` 와 동일 패턴. VTE 파싱은 전용
    /// parser thread(ADR-0002)가 PTY 바이트 도착 즉시 처리하므로, 게이트를
    /// "다음 process_surface 호출까지" 지연시키면 그 사이 도착한 매칭 출력이
    /// 게이트 OFF 상태로 파싱되어 이벤트가 유실된다 — 등록 즉시 게이트를 열어야
    /// 회귀 없이 fire 된다.
    pub(crate) fn register_surface_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        event: tasty_hooks::HookEvent,
        binding: tasty_hooks::HookBinding,
        once: bool,
    ) -> u64 {
        let id = engine
            .hook_manager
            .add_hook(surface_id, event, binding, once);
        engine.sync_output_event_gates();
        id
    }

    /// surface hook 해제. 반환: 실제 제거 여부. 게이트 동기화 이유는
    /// [`Core::register_surface_hook`] 참고.
    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        hook_id: u64,
    ) -> bool {
        let removed = engine.hook_manager.remove_hook(hook_id);
        engine.sync_output_event_gates();
        removed
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
                &f.received,
                surface_id,
            );
            ids.push(f.hook_id);
        }
        ids
    }

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

    /// Hub 가 IPC 서버를 시작한 직후 1회 호출. 두 번째 호출 부터는 무시.
    pub(crate) fn set_host_ipc_injector(&self, injector: crate::ipc::host_call::HostIpcInjector) {
        if self.host_ipc_injector.set(injector).is_err() {
            tracing::warn!("host_ipc_injector already initialized");
        }
    }

    /// Arc<OnceLock<HostIpcInjector>> 의 사본. 메인 스레드가 아닌 별도 스레드가
    /// 자체적으로 host IPC 를 재주입할 때 쓴다 — `terminal.tell`/`terminal.spawn`
    /// 의 제출 `\r` ack 대기 스레드(`adapters::ipc::handler::terminal`)가 소비.
    pub(crate) fn host_ipc_injector_arc(
        &self,
    ) -> Arc<OnceLock<crate::ipc::host_call::HostIpcInjector>> {
        self.host_ipc_injector.clone()
    }

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
            hook_task_waits: self.hook_task_waits.clone(),
        }
    }

    /// 결정 2: 부팅 경로(headless `boot.rs` / GUI `boot_machine.rs`)가 host IPC
    /// injector 등록 + `CoreState` 확보 직후 1 회 호출한다. 자동 시작은 하지
    /// 않는다(결정 1) — 라이브 workspace 전부의 재시작 정화(stale semaphore/
    /// lease holder 회수, persisted handle reload, `Running`→`Failed("host
    /// restart")` 마감)만 runner thread 없이 수행해, 사용자가 나중에
    /// `agent.task_run --action start` 로 수동 재개할 때 유령 상태를 만나지
    /// 않게 한다.
    pub(crate) fn purge_stale_agent_state_on_boot(&self, engine: &crate::core::CoreState) {
        let ctx = self.runner_context(engine);
        let workspace_ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
        crate::core::agent::runner_thread::purge_stale_agent_state_on_boot(&ctx, &workspace_ids);
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
        let mut guard =
            crate::poison::recover_mutex(self.memory.lock(), MEMORY_WHAT, &MEMORY_POISONED);
        f(&mut *guard)
    }

    /// Notification sound player port 참조. cascade 가 `settings.notification.sound`
    /// gate 통과 시 `play()` 호출.
    #[cfg(feature = "gui")]
    pub(crate) fn sound_player(&self) -> &Arc<dyn NotificationSoundPlayer> {
        &self.sound_player
    }
}
