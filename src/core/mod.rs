//! Core는 외부 기능·저장소 구현을 공유하고 메서드에 대상 CoreState를 받는다.
//! GUI는 창별 상태를, 헤드리스는 부팅한 상태를 가진다. 창 연산은 도메인이 선언한
//! cascade_window·identify_port로 요청해 창 구현에 직접 의존하지 않게 한다.

pub(crate) mod agent;
pub(crate) mod attach;
#[cfg(feature = "gui")]
pub(crate) mod attach_mesh_frames;
#[cfg(feature = "gui")]
pub(crate) mod attach_readonly;
pub(crate) mod attach_runtime;
pub(crate) mod attach_structure_sync;
pub(crate) mod builder;
pub(crate) mod bulk_transfer;
pub(crate) mod capture_upload;
pub(crate) mod cascade_window;
pub(crate) mod child_terminal;
pub(crate) mod command_index;
pub(crate) mod egui_mesh_surface;
#[cfg(feature = "gui")]
pub(crate) mod explorer_favorites;
pub(crate) mod file;
pub(crate) mod fs_list;
pub(crate) mod hook_event_registry;
pub(crate) mod host_event;
#[cfg(feature = "gui")]
pub(crate) mod identify_port;
pub(crate) mod intent;
pub(crate) mod ipc_facade;
pub(crate) mod layout_persistence;
pub(crate) mod mesh_mirror;
pub(crate) mod origin;
pub(crate) mod output_observer;
pub(crate) mod param_bag;
#[cfg(feature = "gui")]
pub(crate) mod port_favorites;
pub(crate) mod pty_registry;
pub(crate) mod restore_rebuild;
pub(crate) mod session;
pub(crate) mod state;
pub(crate) mod structural_cascade;
pub(crate) mod structural_exec;
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

#[cfg(feature = "gui")]
pub(crate) use state::{AttachMeshContextForward, GuiAttachUserReq, PendingImageUpload};
pub(crate) use state::{AttentionKind, CoreState};

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
    MirrorStructuralBlocked, PendingStructuralForward, mark_last_forward_agent_origin,
    mark_last_forward_user_triggered,
};
pub(crate) use impl_workspace::{WorkspaceCreationParams, apply_create_workspace_inner};

#[cfg(feature = "gui")]
static NEXT_LIST_DIR_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[cfg(feature = "gui")]
pub(crate) fn next_list_dir_request_id() -> u64 {
    NEXT_LIST_DIR_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(feature = "gui")]
static NEXT_GIT_QUERY_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[cfg(feature = "gui")]
pub(crate) fn next_git_query_request_id() -> u64 {
    NEXT_GIT_QUERY_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// markdown 요청 ID는 1부터 시작한다. 0은 대기 요청 취소 신호지만 카운터 overflow는 별도로 막지 않는다.
#[cfg(feature = "gui")]
static NEXT_MARKDOWN_CONTENT_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[cfg(feature = "gui")]
pub(crate) fn next_markdown_content_request_id() -> u64 {
    NEXT_MARKDOWN_CONTENT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// file_picker.trigger 왕복의 ID. popup 내부에서 디렉터리를 조회하는 list_dir 요청 ID와 별개다.
#[cfg(feature = "gui")]
static NEXT_FILE_PICKER_TRIGGER_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[cfg(feature = "gui")]
pub(crate) fn next_file_picker_trigger_request_id() -> u64 {
    NEXT_FILE_PICKER_TRIGGER_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// memory port와 같은 poison 보고 플래그를 공유해 같은 락의 오류를 여러 번 기록하지 않는다.
pub(crate) use tasty_memory::{
    STORE_LOCK_POISONED as MEMORY_POISONED, STORE_LOCK_WHAT as MEMORY_WHAT,
};

/// preset_store와 presets가 같은 Arc를 공유하므로 poison 보고 플래그도 하나를 쓴다.
pub(crate) const PRESET_STORE_WHAT: &str = "preset store";
pub(crate) static PRESET_STORE_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// App이 attach 채널로 보낼 디렉터리 조회 요청.
#[derive(Debug, Clone)]
#[cfg(feature = "gui")]
pub(crate) struct PendingListDirForward {
    pub(crate) local_ws_id: u32,
    pub(crate) request_id: u64,
    pub(crate) dir: String,
    /// None이면 파일 선택기, Some이면 해당 explorer surface로 응답을 전달한다.
    pub(crate) consumer: Option<u32>,
}

/// App이 attach 채널로 보낼 Git 조회 요청.
#[derive(Debug, Clone)]
#[cfg(feature = "gui")]
pub(crate) struct PendingGitQueryForward {
    /// 원격 ID로 바꿀 로컬 mirror surface. worktree_path가 없으면 서버가 해당 터미널 cwd를 조회한다.
    pub(crate) local_surface_id: u32,
    pub(crate) request_id: u64,
    pub(crate) kind: tasty_ipc::stream_hub::GitQueryKind,
    /// 서버 응답에서 받은 경로. client의 로컬 경로로 해석하지 않는다.
    pub(crate) worktree_path: Option<String>,
    /// Diff 요청에만 사용한다.
    pub(crate) diff_path: Option<String>,
}

/// markdown 원문 조회 요청. plugin이 surface별 대기를 추적하고 응답에도 surface ID가 포함된다.
#[derive(Debug, Clone)]
#[cfg(feature = "gui")]
pub(crate) struct PendingMarkdownContentForward {
    /// 원문을 기다리는 로컬 mirror surface ID.
    pub(crate) local_surface_id: u32,
    pub(crate) request_id: u64,
    /// 에이전트 요청의 잘린 응답은 사용자 toast로 알리지 않는다.
    pub(crate) agent_origin: bool,
}

/// 프로세스가 공유하는 port와 저장소 핸들. 창별 데이터는 CoreState에 있다.
#[allow(dead_code)] // 이유: 일부 port는 아직 읽지 않지만 CoreBuilder가 같은 port 묶음을 주입하는 인터페이스를 유지한다.
pub(crate) struct Core {
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    clipboard: Arc<dyn ClipboardSystem>,
    process: Arc<dyn ProcessSpawner>,
    home: Arc<dyn HomeDirectory>,
    sound_player: Arc<dyn NotificationSoundPlayer>,

    /// MemoryStorage는 Sync를 요구하지 않아 Mutex로 보호한다.
    memory: Arc<Mutex<dyn MemoryStorage>>,
    themes: Arc<dyn ThemeStorage>,
    /// 변경 메서드 때문에 Mutex가 필요하다. preset_store와 같은 Arc다.
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,

    /// 뷰에도 같은 Arc를 넘겨 preset 디스크 캐시를 공유한다.
    pub(crate) preset_store: Arc<Mutex<PresetStore>>,

    /// IPC 서버 시작 뒤 주입한다. runner 등 다른 스레드가 host IPC를 호출할 때 쓴다.
    pub(crate) host_ipc_injector: Arc<OnceLock<tasty_ipc::host_call::HostIpcInjector>>,

    pub(crate) runner_registry: Arc<crate::core::agent::runner_thread::RunnerRegistry>,

    pub(crate) hook_task_waits: Arc<crate::core::agent::hook_wait::HookTaskWaits>,

    /// GUI·헤드리스 큐와 핸들러가 같은 요청 압력 계측을 사용한다.
    pressure: tasty_telemetry::PressureStats,

    gate: tasty_telemetry::GateStats,

    /// Core를 참조할 수 없는 plugin 통신 크레이트에도 같은 계측 Arc를 넘긴다.
    plugin_wait: Arc<tasty_telemetry::PluginWaitStats>,

    /// 호스트와 plugin 요청의 느린 처리 기록을 공유한다.
    slow_requests: Arc<tasty_telemetry::SlowRequestLog>,

    /// 진단 조회가 MemoryStorage mutex를 잡지 않도록 저장소의 계측 Arc를 별도로 공유한다.
    /// 주입되지 않은 기본 계측기는 관측값이 비어 있다.
    db_latency: Arc<tasty_memory::DbLatencyStats>,

    /// Core 뒤에 시작하는 IPC 서버에 넘길 연결 계측기.
    connections: Arc<tasty_telemetry::ConnectionStats>,

    dispatch: Arc<tasty_ipc::dispatch::DispatchStats>,

    /// 저장소를 열 때 읽은 pragma 결과 사본. 진단이 저장소 mutex를 잡지 않게 한다. 없으면 None이다.
    memory_pragmas: Option<tasty_memory::pragma::AppliedPragmas>,

    /// 초기화 실패 뒤 대체 저장소를 쓴 사유. None만으로 실제 쓰기 내구성을 검증할 수는 없다.
    memory_init_fallback: Option<tasty_memory::InitFallback>,
}

/// GUI·헤드리스의 plugin 매니저에 함께 주입할 계측 핸들.
#[derive(Clone)]
pub(crate) struct PluginGauges {
    pub(crate) plugin_wait: Arc<tasty_telemetry::PluginWaitStats>,
    pub(crate) slow_requests: Arc<tasty_telemetry::SlowRequestLog>,
}

impl Core {
    pub(crate) fn now_instant(&self) -> std::time::Instant {
        self.clock.now_instant()
    }

    pub(crate) fn pressure(&self) -> &tasty_telemetry::PressureStats {
        &self.pressure
    }

    pub(crate) fn gate(&self) -> &tasty_telemetry::GateStats {
        &self.gate
    }

    pub(crate) fn plugin_wait(&self) -> &Arc<tasty_telemetry::PluginWaitStats> {
        &self.plugin_wait
    }

    pub(crate) fn slow_requests(&self) -> &Arc<tasty_telemetry::SlowRequestLog> {
        &self.slow_requests
    }

    pub(crate) fn plugin_gauges(&self) -> PluginGauges {
        PluginGauges {
            plugin_wait: self.plugin_wait.clone(),
            slow_requests: self.slow_requests.clone(),
        }
    }

    pub(crate) fn db_latency(&self) -> &Arc<tasty_memory::DbLatencyStats> {
        &self.db_latency
    }

    pub(crate) fn memory_pragmas(&self) -> Option<&tasty_memory::pragma::AppliedPragmas> {
        self.memory_pragmas.as_ref()
    }

    pub(crate) fn memory_init_fallback(&self) -> Option<&tasty_memory::InitFallback> {
        self.memory_init_fallback.as_ref()
    }

    pub(crate) fn connections(&self) -> &Arc<tasty_telemetry::ConnectionStats> {
        &self.connections
    }

    pub(crate) fn dispatch(&self) -> &Arc<tasty_ipc::dispatch::DispatchStats> {
        &self.dispatch
    }

    /// 감사·보관 기간 계산은 벽시계를 사용하므로 monotonic 시각과 별도로 제공한다.
    pub(crate) fn now_unix_millis(&self) -> i64 {
        self.clock.now_unix_millis()
    }

    pub(crate) fn send_surface_message(
        &mut self,
        engine: &mut crate::core::CoreState,
        from: u32,
        to: u32,
        content: String,
    ) -> u32 {
        engine.send_message(from, to, content)
    }

    pub(crate) fn read_surface_messages(
        &mut self,
        engine: &mut crate::core::CoreState,
        sid: u32,
        from: Option<u32>,
        peek: bool,
    ) -> Vec<state::SurfaceMessage> {
        engine.read_messages(sid, from, peek)
    }

    pub(crate) fn clear_surface_messages(&mut self, engine: &mut crate::core::CoreState, sid: u32) {
        engine.clear_messages(sid);
    }

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

    pub(crate) fn observer_unregister(
        &mut self,
        engine: &mut crate::core::CoreState,
        observer_id: u64,
    ) -> Result<(), crate::output_observer::ObserverError> {
        engine.observer_router.unregister(observer_id)?;
        engine.sync_output_event_gates();
        Ok(())
    }

    pub(crate) fn observer_list(
        &self,
        engine: &crate::core::CoreState,
    ) -> Vec<crate::output_observer::ObserverInfo> {
        engine.observer_router.list()
    }

    pub(crate) fn observer_info(
        &self,
        engine: &crate::core::CoreState,
        observer_id: u64,
    ) -> Option<crate::output_observer::ObserverInfo> {
        engine.observer_router.info(observer_id)
    }

    /// hook을 등록한 직후 출력 이벤트 허용 상태도 맞춘다.
    /// parser 스레드가 이미 출력 처리 중이므로 다음 process_surface까지 미루면 중간 출력 이벤트를 놓칠 수 있다.
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

    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        hook_id: u64,
    ) -> bool {
        let removed = engine.hook_manager.remove_hook(hook_id);
        engine.sync_output_event_gates();
        removed
    }

    pub(crate) fn register_global_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        condition: crate::global_hooks::HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        engine.global_hook_manager.add(condition, command, label)
    }

    pub(crate) fn unregister_global_hook(
        &mut self,
        engine: &mut crate::core::CoreState,
        hook_id: u32,
    ) -> bool {
        engine.global_hook_manager.remove(hook_id)
    }

    /// 일치한 hook의 바인딩을 실행하고 ID를 반환한다. host 이벤트를 큐에 넣는 일은 호출자가 맡는다.
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

    pub(crate) fn request_approval(
        &mut self,
        engine: &mut crate::core::CoreState,
        req: tasty_approval::ApprovalRequest,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.request(req)
    }

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

    pub(crate) fn cancel_approval(
        &mut self,
        engine: &mut crate::core::CoreState,
        req_id: &tasty_approval::ApprovalId,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.cancel(req_id)
    }

    /// Core를 직접 받지 않는 뷰·정리 코드에도 같은 저장소를 주입하기 위한 Arc 사본.
    pub(crate) fn memory_arc(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        self.memory.clone()
    }

    /// 캡처 worker와 원격 클립보드 처리에도 같은 구현을 주입한다.
    pub(crate) fn clipboard_arc(&self) -> Arc<dyn ClipboardSystem> {
        self.clipboard.clone()
    }

    /// IPC 서버 시작 뒤 한 번 주입한다. 두 번째 요청은 경고만 남긴다.
    pub(crate) fn set_host_ipc_injector(&self, injector: tasty_ipc::host_call::HostIpcInjector) {
        if self.host_ipc_injector.set(injector).is_err() {
            tracing::warn!("host_ipc_injector already initialized");
        }
    }

    pub(crate) fn host_ipc_injector_arc(
        &self,
    ) -> Arc<OnceLock<tasty_ipc::host_call::HostIpcInjector>> {
        self.host_ipc_injector.clone()
    }

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

    /// 현재 engine의 workspace에 남은 runner 상태를 정리한다. runner 스레드를 자동 시작하지는 않는다.
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

    /// 렌더링 등 Core를 받지 않는 코드가 같은 runner 상태를 조회하도록 Arc를 주입한다.
    /// OnceLock이 이미 차 있으면 덮어쓰지 않고 경고한다.
    pub(crate) fn inject_agent_runner_registry(&self, engine: &crate::core::CoreState) {
        if engine
            .agent_runner_registry
            .set(self.agent_runner_registry())
            .is_err()
        {
            tracing::warn!("agent runner registry already injected into CoreState");
        }
    }

    /// 공용 poison 복구 헬퍼로 저장소 락을 얻고 콜백을 실행한다. 콜백이 끝날 때까지 락을 유지한다.
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard =
            crate::poison::recover_mutex(self.memory.lock(), MEMORY_WHAT, &MEMORY_POISONED);
        f(&mut *guard)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn sound_player(&self) -> &Arc<dyn NotificationSoundPlayer> {
        &self.sound_player
    }
}
