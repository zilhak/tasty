//! Process-wide application services and execution coordination. Engine structure has another owner.
use crate::app::command::{CoreEvent, DomainIntent, ProcessPtyOutcome};
use crate::core::*;
use crate::ports::{
    clipboard::ClipboardSystem, clock::Clock, fs::FileSystem, home::HomeDirectory,
    notification_sound::NotificationSoundPlayer, process::ProcessSpawner,
};
use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::sync::{Arc, Mutex, OnceLock};
use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;
pub(crate) mod builder;
pub(crate) mod file;
pub(crate) mod impl_attach;
pub(crate) mod impl_clipboard;
#[cfg(feature = "gui")]
pub(crate) mod impl_mirror;
pub(crate) mod impl_pty;
pub(crate) mod impl_tab;
pub(crate) mod ipc_facade;
mod live_intent;
pub(crate) mod session;
/// 프로세스가 공유하는 port와 저장소 핸들. 창별 데이터는 CoreState에 있다.
#[allow(dead_code)] // 이유: 일부 port는 아직 읽지 않지만 AppServicesBuilder가 같은 port 묶음을 주입하는 인터페이스를 유지한다.
pub(crate) struct AppServices {
    pub(crate) registries: crate::runtime::registries::RuntimeRegistries,
    /// Process-wide services. Engine and View owners carry neither copies nor lookup authority.
    #[cfg(feature = "gui")]
    pub(crate) navigation_proofs: Arc<crate::app::html_runtime::NavigationProofs>,
    /// 모든 창이 함께 쓰는 Explorer 복사·잘라내기 목록. 창의 View 상태를 만들 때 같은 슬롯을 넘긴다.
    #[cfg(feature = "gui")]
    pub(crate) explorer_clipboard: crate::state::explorer_clipboard_slot::ExplorerClipboardSlot,
    /// OS 파일 클립보드. 창마다 같은 핸들을 넘기고, 창이 닫힌 뒤 끝난 잘라내기도 이것으로 정리한다.
    #[cfg(feature = "gui")]
    pub(crate) file_clipboard: crate::clipboard::files::Handle,
    pub(crate) profile_detections: tasty_remote::profile_detection::ProfileDetections,
    pub(crate) approval_store: Arc<tasty_approval::ApprovalStore>,
    pub(crate) telemetry_seq: Arc<tasty_telemetry::TelemetrySeq>,
    pub(crate) anomaly_detector: Arc<tasty_telemetry::AnomalyDetector>,
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

    /// 이 인스턴스가 bind한 IPC·웹훅 포트. 포트가 필요한 곳은 여기서 읽는다.
    pub(crate) ports: Arc<crate::runtime_ports::RuntimePorts>,

    /// 러너·완료 대기·훅-작업 연결을 가진 작업 실행 서비스.
    pub(crate) tasks: tasty_task_runtime::TaskService,

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

    /// AppServices 뒤에 시작하는 IPC 서버에 넘길 연결 계측기.
    connections: Arc<tasty_telemetry::ConnectionStats>,

    dispatch: Arc<tasty_ipc::dispatch::DispatchStats>,

    /// 저장소를 열 때 읽은 pragma 결과 사본. 진단이 저장소 mutex를 잡지 않게 한다. 없으면 None이다.
    memory_pragmas: Option<tasty_memory::pragma::AppliedPragmas>,

    /// 초기화 실패 뒤 대체 저장소를 쓴 사유. None만으로 실제 쓰기 내구성을 검증할 수는 없다.
    memory_init_fallback: Option<tasty_memory::InitFallback>,

    /// memory.db 전담 writer. 부팅 구성에서만 만들며 시험 구성에는 없다. 이행 중에는 `memory`와
    /// 같은 잠금을 묶음마다 잡는다(docs/design/systems/storage-contract.md의 "저장 실행기" 절).
    memory_writer: Option<HostMemoryWriter>,
    /// memory.db 요청 핸들. writer가 있으면 그 큐로, 없으면(시험 구성) 호출 스레드에서 바로 실행한다.
    memory_access: tasty_memory::writer::MemoryAccess,
    /// 예상하지 못한 writer 종료를 한 번만 알린다.
    memory_writer_end_reported: bool,
}

/// 호스트의 memory.db writer. 이행 중에는 공유 저장소 잠금으로 저장소에 닿는다.
pub(crate) type HostMemoryWriter =
    tasty_memory::writer::MemoryWriter<tasty_memory::writer::SharedStore>;

/// GUI·헤드리스의 plugin 매니저에 함께 주입할 계측 핸들.
#[derive(Clone)]
pub(crate) struct PluginGauges {
    pub(crate) plugin_wait: Arc<tasty_telemetry::PluginWaitStats>,
    pub(crate) slow_requests: Arc<tasty_telemetry::SlowRequestLog>,
}

impl AppServices {
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

    #[cfg(test)]
    pub(crate) fn memory_access(&self) -> &tasty_memory::writer::MemoryAccess {
        &self.memory_access
    }

    /// commit된 regular 변경을 접수 순서대로 꺼낸다. writer가 옮긴 변경 뒤에 아직 직접 잠금
    /// 소비자가 남긴 변경을 붙인다.
    pub(crate) fn take_memory_changes(&self) -> Vec<tasty_memory::MemoryChange> {
        let mut changes = self.memory_access.changes().take();
        changes.extend(self.with_memory(|s| s.take_pending_changes()));
        changes
    }

    /// writer 계측. writer가 없는 구성이면 None이다.
    pub(crate) fn memory_writer_stats(
        &self,
    ) -> Option<tasty_utils::batch_writer::WriterStatsSnapshot> {
        self.memory_writer.as_ref().map(|w| w.stats())
    }

    /// 메인 루프가 writer를 깨운 뒤 부른다. 종료를 요청하지 않았는데 worker가 끝났으면 한 번 알린다.
    pub(crate) fn poll_memory_writer(&mut self) {
        if !self.memory_writer_end_reported
            && self
                .memory_writer
                .as_ref()
                .is_some_and(|w| w.worker_ended_unexpectedly())
        {
            self.memory_writer_end_reported = true;
            tracing::error!("memory writer ended without a shutdown request");
        }
    }

    /// 접수를 멈추고 마지막 flush 뒤 worker를 끝내도록 하고 남은 worker 수를 돌려준다.
    /// 다른 producer를 모두 멈춘 뒤 부른다. 기다리지 않는다.
    pub(crate) fn poll_memory_writer_shutdown(&mut self) -> usize {
        self.memory_writer
            .as_mut()
            .map_or(0, HostMemoryWriter::poll_shutdown)
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
        engine: &mut EngineMut<'_>,
        from: u32,
        to: u32,
        content: String,
    ) -> u32 {
        engine.send_message(from, to, content)
    }

    pub(crate) fn read_surface_messages(
        &mut self,
        engine: &mut EngineMut<'_>,
        sid: u32,
        from: Option<u32>,
        peek: bool,
    ) -> Vec<state::SurfaceMessage> {
        engine.read_messages(sid, from, peek)
    }

    pub(crate) fn clear_surface_messages(&mut self, engine: &mut EngineMut<'_>, sid: u32) {
        engine.clear_messages(sid);
    }

    pub(crate) fn observer_register(
        &mut self,
        engine: &mut EngineMut<'_>,
        spec: crate::output_observer::ObserverSpec,
    ) -> Result<u64, crate::output_observer::ObserverError> {
        let id = engine
            .observer_router
            .register(spec, self.memory_access.clone())?;
        engine.sync_output_event_gates();
        Ok(id)
    }

    pub(crate) fn observer_unregister(
        &mut self,
        engine: &mut EngineMut<'_>,
        observer_id: u64,
    ) -> Result<(), crate::output_observer::ObserverError> {
        engine.observer_router.unregister(observer_id)?;
        engine.sync_output_event_gates();
        Ok(())
    }

    pub(crate) fn observer_list(
        &self,
        engine: &EngineRef<'_>,
    ) -> Vec<crate::output_observer::ObserverInfo> {
        engine.observer_router.list()
    }

    pub(crate) fn observer_info(
        &self,
        engine: &EngineRef<'_>,
        observer_id: u64,
    ) -> Option<crate::output_observer::ObserverInfo> {
        engine.observer_router.info(observer_id)
    }

    /// hook을 등록한 직후 출력 이벤트 허용 상태도 맞춘다.
    /// parser 스레드가 이미 출력 처리 중이므로 다음 process_surface까지 미루면 중간 출력 이벤트를 놓칠 수 있다.
    pub(crate) fn register_surface_hook(
        &mut self,
        engine: &mut EngineMut<'_>,
        surface_id: u32,
        event: tasty_hooks::HookEvent,
        binding: tasty_hooks::HookBinding,
        once: bool,
    ) -> u64 {
        let id = engine
            .hooks
            .add_surface_hook(surface_id, event, binding, once);
        engine.sync_output_event_gates();
        id
    }

    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut EngineMut<'_>,
        hook_id: u64,
    ) -> bool {
        let removed = engine.hooks.remove_surface_hook(hook_id);
        engine.sync_output_event_gates();
        removed
    }

    pub(crate) fn register_global_hook(
        &mut self,
        engine: &mut EngineMut<'_>,
        condition: crate::global_hooks::HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        engine.hooks.add_global_hook(condition, command, label)
    }

    pub(crate) fn unregister_global_hook(
        &mut self,
        engine: &mut EngineMut<'_>,
        hook_id: u32,
    ) -> bool {
        engine.hooks.remove_global_hook(hook_id)
    }

    /// 발화한 훅 바인딩의 실행기. IPC 주입기가 아직 없으면 IpcSequence handler를 건너뛴다.
    pub(crate) fn hook_executor(&self) -> crate::hook_runtime::HookExecutor {
        crate::hook_runtime::HookExecutor::new(self.host_ipc_injector.get().cloned())
    }

    pub(crate) fn request_approval(
        &mut self,
        req: tasty_approval::ApprovalRequest,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        self.approval_store.request(req)
    }

    pub(crate) fn respond_approval(
        &mut self,
        req_id: &tasty_approval::ApprovalId,
        choice: String,
        by: tasty_approval::Responder,
        comment: Option<String>,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        self.approval_store.respond(req_id, choice, by, comment)
    }

    pub(crate) fn cancel_approval(
        &mut self,
        req_id: &tasty_approval::ApprovalId,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        self.approval_store.cancel(req_id)
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

    /// IPC 서버 bind 뒤 한 번 기록한다. IPC 서버는 loopback의 OS 지정 포트를 쓴다.
    pub(crate) fn record_ipc_port(&self, port: u16) {
        self.ports.record_ipc(crate::runtime_ports::BoundPort {
            addr: std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            source: crate::runtime_ports::PortSource::Dynamic,
        });
    }

    /// 자기 자신을 향한 attach 요청을 거절할 때 쓴다. IPC 서버가 없으면 None이다.
    pub(crate) fn own_ipc_port(&self) -> Option<u16> {
        self.ports.ipc_port()
    }

    pub(crate) fn host_ipc_injector_arc(
        &self,
    ) -> Arc<OnceLock<tasty_ipc::host_call::HostIpcInjector>> {
        self.host_ipc_injector.clone()
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

mod barrier;
mod lease;
mod ratelimit;
mod semaphore;

mod surface;
#[cfg(feature = "gui")]
pub(crate) use surface::RemoteConnParams;
#[cfg(feature = "gui")]
pub(crate) use surface::no_application_state;
pub(crate) use surface::{clipboard_set_text, spawn_remote_workspaces};
#[cfg(debug_assertions)]
mod surface_debug;
#[cfg(debug_assertions)]
pub(crate) use surface_debug::{fullscreen_list, lua_eval};
