use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::surface_registry::SurfaceKindRegistry;
use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::state::SurfaceMessage;
use tasty_hooks::HookManager;
use tasty_terminal::Waker;

/// ID generator for workspaces, panes, tabs, and surfaces.
///
/// 각 카운터는 `Arc<AtomicU32>` 로, 여러 CoreState 가 같은 ID 공간을 공유한다.
/// multi-window 시 두 engine 이 동일 ID 를 발급하면 IPC routing 이 불정확해지므로
/// 글로벌 유니크가 필요. `Clone` 으로 새 engine 에 같은 Arc 를 넘긴다.
#[derive(Clone)]
pub struct IdGenerator {
    workspace: Arc<std::sync::atomic::AtomicU32>,
    pane: Arc<std::sync::atomic::AtomicU32>,
    tab: Arc<std::sync::atomic::AtomicU32>,
    surface: Arc<std::sync::atomic::AtomicU32>,
}

impl IdGenerator {
    pub fn new() -> Self {
        use std::sync::atomic::AtomicU32;
        Self {
            workspace: Arc::new(AtomicU32::new(1)),
            pane: Arc::new(AtomicU32::new(1)),
            tab: Arc::new(AtomicU32::new(1)),
            surface: Arc::new(AtomicU32::new(1)),
        }
    }

    pub fn next_workspace(&self) -> u32 {
        self.workspace
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_pane(&self) -> u32 {
        self.pane.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_tab(&self) -> u32 {
        self.tab.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_surface(&self) -> u32 {
        self.surface
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

/// Helper to extract shell configuration from settings, avoiding boilerplate.
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
}

impl ShellConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            shell: settings.general.shell.clone(),
            args: settings.general.effective_shell_args(),
        }
    }

    pub fn shell_ref(&self) -> Option<&str> {
        if self.shell.is_empty() {
            None
        } else {
            Some(&self.shell)
        }
    }

    pub fn args_ref(&self) -> Vec<&str> {
        self.args.iter().map(|s| s.as_str()).collect()
    }
}

/// Engine-level state shared across all windows.
/// Contains all data that is not specific to a single window's UI.
///
/// struct 자체는 `pub` — pub fn 시그니처에 노출되기 때문 (e.g. `AppState::active_workspace`).
/// 다만 모든 필드는 `pub(crate)` 로 좁혀, 외부 (crate dependency 측) 가 내부
/// 도메인 데이터를 직접 두드리는 것을 막는다. Core boundary 강화.
pub struct CoreState {
    // ── Workspace / Terminal management ──
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) next_ids: IdGenerator,
    pub(crate) default_cols: usize,
    pub(crate) default_rows: usize,
    pub(crate) waker: Waker,

    // ── Settings ──
    pub(crate) settings: Settings,

    // ── Notifications / Hooks ──
    pub(crate) notifications: NotificationStore,
    pub(crate) hook_manager: HookManager,
    pub(crate) global_hook_manager: GlobalHookManager,

    // ── Closed item history ──
    pub(crate) closed_items: crate::model::ClosedItemStore,

    // ── System clipboard history (memory-only) ──
    pub(crate) clipboard_history: crate::clipboard_history::ClipboardHistory,

    /// OSC 133 기반 명령 인덱서. PromptBoundary 이벤트가 도달할 때마다 호스트가
    /// 호출해 per-surface 상태를 업데이트하고, D phase 에서 memory 에 record 영속.
    pub(crate) command_index: crate::engine::command_index::CommandIndex,

    /// 출력 옵저버 라우터. OutputAppended 이벤트마다 dispatch 호출.
    pub(crate) observer_router: crate::output_observer::ObserverRouter,

    /// 휴먼 핸드오프 — approval 요청/응답 큐 + 대기자 채널.
    pub(crate) approval_store: std::sync::Arc<tasty_approval::ApprovalStore>,

    /// Telemetry 이벤트 시퀀스 — 같은 ms 안에서 event_key 충돌 방지용 단조 증가 카운터.
    pub(crate) telemetry_seq: std::sync::Arc<tasty_telemetry::TelemetrySeq>,

    /// Telemetry 이상 탐지 — 호스트 singleton. in-memory sliding window 만 보관
    /// (Phase 4.4). 검출된 anomaly 레코드는 호스트가 memory store 에 영속.
    pub(crate) anomaly_detector: std::sync::Arc<tasty_telemetry::AnomalyDetector>,

    /// Agent task ID 시퀀스 — 같은 ms 안에서 task_id 충돌 방지용 단조 증가 카운터.
    pub(crate) agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,

    // ── Messaging / Typing detection ──
    pub(crate) surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub(crate) last_key_input: HashMap<u32, std::time::Instant>,

    // ── Busy state cache (foreground process != shell). Updated by BusyPoll.
    // Set membership = busy. Surfaces missing from the set are treated as idle.
    pub(crate) busy_surfaces: std::collections::HashSet<u32>,

    /// Targeted waker creation. winit `EventLoopProxy`를 직접 들지 않고 trait 뒤로
    /// 추상화하여 헤드리스/플러그인 호스트 컨텍스트에서도 동일 인터페이스를 쓴다.
    /// `App`이 CoreState 생성 후 본체에서 `WinitWakerFactory`를 주입한다.
    pub(crate) waker_factory: Option<crate::waker::SharedWakerFactory>,

    // ── CWD polling (round-robin) ──
    // macOS/Linux 전용. Windows에서는 폴링을 돌지 않아 필드 자체가 없음.
    // ── Surface kind registry ──
    /// Surface 종류별 메타·동작 lookup. 단계 03C에서는 빈 레지스트리만 보유한다 —
    /// 03D에서 본체 7종이 등록되며, 단계 05에서 plugin이 추가될 예정.
    pub(crate) surface_registry: Arc<SurfaceKindRegistry>,

    // ── File format / handler registries (file-handler-system) ──
    /// 파일 식별기 — host default + plugin contribute + user config 통합.
    /// `PluginManager` 와 같은 Arc 를 공유한다.
    pub(crate) file_format: Arc<crate::file::format::FileFormatRegistry>,
    /// 파일 핸들러 디스패치 테이블. `PluginManager` 와 같은 Arc 를 공유한다.
    pub(crate) file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    /// 사용자가 picker 에서 직접 고른 handler 의 LRU 기록 (보조 신호).
    /// 부팅 시 디스크에서 로드, 매 선택마다 atomic save.
    pub(crate) file_handler_recent: crate::file::handler::recent::RecentPicks,
    /// 비동기 파일 식별 worker. `App` 이 EventLoopProxy 를 가진 시점에
    /// `create_app_state` 에서 주입한다 — waker_factory 와 동일 패턴.
    /// Phase C 의 mouse.rs 콜사이트가 이걸 호출해 deep identify 를 띄운다.
    pub(crate) identify_worker: Option<std::sync::Arc<crate::identify_worker::IdentifyWorker>>,

    // ── Layout persistence ──
    pub(crate) layout_dirty: crate::engine::layout_persistence::LayoutDirtyTracker,
    /// Active workspace index restored from layout.json. Consumed once by AppState::new().
    pub(crate) restored_active_workspace: Option<usize>,
    /// Deferred terminal surface 의 scrollback 복원 대기 큐. 값은
    /// `scrollback_store::read` 결과(없으면 entry 자체가 생략됨). PTY 가
    /// 실제로 spawn 된 직후 (`ensure_surface_initialized` 또는 즉시 복원
    /// 경로) entry 를 꺼내 `inject_scrollback` 호출.
    pub(crate) pending_scrollback_inject: HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,
    /// 첫 plugin pump 후 적용할 layout. plugin이 제공하는 surface kind가
    /// 등록되기 전에 복원하면 사라지므로 한 번 미뤄둔다. `App::apply_pending_layout_restore`가 소비.
    pub(crate) pending_layout_restore: Option<crate::engine::layout_persistence::SavedLayout>,

    /// Whether input simulation IPC is enabled (debug builds only, --enable-input-simulation).
    #[cfg(debug_assertions)]
    pub(crate) input_simulation_enabled: bool,

    /// Memory port 의 Arc clone — Core 가 owner. `CoreState::new` 직후에는 빈
    /// 상태이며 `App::create_app_state` 가 `core.memory_arc()` 로 주입한다.
    /// engine 내부 (SurfaceMetaStore, layout persistence, pty surface init 등)
    /// cascade 없이 직접 영속할 때 사용.
    pub(crate) memory: Option<std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>>,
}

impl CoreState {
    /// Create a new CoreState with default settings.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        Self::new_with_ids(cols, rows, waker, None)
    }

    /// 새 CoreState 를 만들 때 기존 ID 공간(Arc<AtomicU32> 들)을 공유받는 변형.
    /// multi-window 시 두 번째 main 의 첫 workspace 가 첫 engine 과 ID 충돌하지
    /// 않게 한다. `shared_ids=None` 이면 새 IdGenerator 로 1부터 시작.
    pub fn new_with_ids(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
    ) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let restore_layout = settings.general.restore_layout;

        // Create engine with empty workspaces first; we'll fill them below.
        let mut engine = Self {
            workspaces: Vec::new(),
            next_ids: shared_ids.unwrap_or_else(IdGenerator::new),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            closed_items: crate::model::ClosedItemStore::new(),
            clipboard_history: crate::clipboard_history::ClipboardHistory::new(100),
            command_index: crate::engine::command_index::CommandIndex::new(),
            observer_router: crate::output_observer::ObserverRouter::new(),
            approval_store: std::sync::Arc::new(tasty_approval::ApprovalStore::new()),
            telemetry_seq: std::sync::Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: std::sync::Arc::new(tasty_telemetry::AnomalyDetector::new()),
            agent_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            busy_surfaces: std::collections::HashSet::new(),
            waker_factory: None,
            surface_registry: {
                let reg = SurfaceKindRegistry::new();
                crate::engine::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            file_format: {
                let reg = crate::file::format::FileFormatRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../file/format/defaults/default-file-format.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler: {
                let reg = crate::file::handler::FileHandlerRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../file/handler/defaults/default-file-handlers.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler_recent: crate::file::handler::recent::RecentPicks::load(
                &file_handler_recent_path(),
            ),
            identify_worker: None,
            layout_dirty: crate::engine::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            memory: None,
        };

        // (Phase E) FileHandler 가 detector 메타 (광고 확장자 등) 를 조회할 수 있게
        // FileFormatRegistry 를 DetectorInfo 로 주입. host default 가 이미 로드된 시점.
        engine
            .file_handler
            .attach_detector_info(engine.file_format.clone());

        // Re-apply coalesce_ms from actual settings
        engine.notifications =
            NotificationStore::with_coalesce_ms(engine.settings.notification.coalesce_ms);

        // Apply clipboard history max from settings.
        engine
            .clipboard_history
            .set_max(engine.settings.clipboard.history_max);

        // Try restoring saved layout. plugin이 제공하는 surface kind(예: explorer)는
        // PluginManager가 hello를 처리한 후에야 registry에 등록되므로, 여기서 즉시
        // 복원하면 그런 surface가 사라진다. 따라서 layout 복원은 첫 plugin pump 후로
        // 지연한다 (`App::apply_pending_layout_restore`).
        let mut restored = false;
        if restore_layout {
            if let Some(saved) = crate::engine::layout_persistence::load_from_disk() {
                // layout.json 에 남아 있는 scrollback_ref 집합 외의 파일은 모두 orphan.
                // capture 도중 크래시했거나 옛 surface 의 잔재이므로 삭제해 디스크 leak 방지.
                let known = saved.collect_scrollback_refs();
                crate::scrollback_store::gc_orphans(&known);
                engine.pending_layout_restore = Some(saved);
                // 첫 화면이 비지 않도록 default workspace는 일단 fallback에서 만들고,
                // pending_layout_restore가 적용될 때 교체된다.
                restored = false;
            } else {
                // layout.json 자체가 없거나 무효면 알려진 ref 없음 → 모두 orphan.
                let empty: std::collections::HashSet<String> = std::collections::HashSet::new();
                crate::scrollback_store::gc_orphans(&empty);
            }
        }

        // Fallback: create default workspace
        if !restored {
            let ws_id = engine.next_ids.next_workspace();
            let pane_id = engine.next_ids.next_pane();
            let tab_id = engine.next_ids.next_tab();
            let surface_id = engine.next_ids.next_surface();
            let sh = ShellConfig::from_settings(&engine.settings);
            let ws = Workspace::new_with_shell(
                ws_id,
                "Workspace 1".to_string(),
                pane_id,
                tab_id,
                surface_id,
                crate::model::ShellSpawnOpts {
                    cols: cols,
                    rows: rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker: waker,
                    working_dir: None,
                },
            )?;
            engine.workspaces = vec![ws];
            engine.send_fast_init(surface_id);
        }

        Ok(engine)
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    /// Create a waker for a terminal. If targeted_pty_polling is enabled,
    /// the waker includes the surface_id so only that terminal is processed.
    /// Otherwise, returns the shared waker (all terminals polled).
    pub fn make_waker(&self, surface_id: u32) -> Waker {
        // targeted_pty_polling이 켜져 있고 factory가 주입되어 있으면 surface별 waker 생성.
        // 그 외에는 CoreState 생성 시 받은 base waker(`TerminalOutput(None)`)를 그대로 공유.
        if self.settings.performance.targeted_pty_polling {
            if let Some(factory) = &self.waker_factory {
                return factory.make_targeted_waker(surface_id);
            }
        }
        self.waker.clone()
    }

    /// Push a closed item, automatically injecting restore commands from surface metadata.
    /// Plugins write the `restore.command` meta key directly (host stays agent-agnostic).
    pub fn push_closed_item(&mut self, mut item: crate::model::ClosedItem) {
        let mem = self.memory.clone();
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            mem.as_ref()
                .and_then(|m| crate::surface_meta::SurfaceMetaStore::get(m, sid, "restore.command"))
        });
        self.closed_items.push(item);
    }

    /// Record that the user typed on the given surface.
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    /// Internally-originated clipboard copy (selection copy 등). 히스토리에 저장하되
    /// `Source::Internal`로 태깅. `history_enabled`가 false면 no-op.
    pub fn record_internal_copy(&mut self, text: &str) {
        if !self.settings.clipboard.history_enabled {
            return;
        }
        self.clipboard_history.record(
            text.to_string(),
            crate::clipboard_history::ClipboardSource::Internal,
        );
    }

    /// Returns true if the surface received key input within the last 5 seconds.
    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    /// 사용자 picker 선택 기록 — 즉시 디스크에 atomic save. 실패 시 warn 로그.
    pub fn record_file_handler_pick(&mut self, id: &crate::file::handler::HandlerId) {
        self.file_handler_recent.record(id);
        let path = file_handler_recent_path();
        if let Err(e) = self.file_handler_recent.save_atomic(&path) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "file_handler_recent: atomic save failed",
            );
        }
    }
}

impl CoreState {
    /// Refresh the cached display name of the tab containing a given surface ID.
    pub fn refresh_tab_display_name(&mut self, surface_id: u32) {
        for workspace in &mut self.workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            tab.refresh_display_name();
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Update stored grid dimensions.
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.default_cols = cols;
        self.default_rows = rows;
    }
}

// CoreReader port — read-only 진입점. handler 가 점진적으로 `&dyn CoreReader`
// 받게 마이그레이션 (D.3.C 의 각 도메인 step).
impl crate::ports::inbound::CoreReader for CoreState {
    fn state(&self) -> &CoreState {
        self
    }
}

/// `~/.tasty/file-handlers.toml` — 사용자 detector/handler 설정. 부팅 시 1회 로드.
fn file_handler_user_config_path() -> Option<std::path::PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("file-handlers.toml"))
}

/// `~/.tasty/file-handler-recent.json` — picker 선택 LRU. 부팅 시 로드, 매 선택마다 save.
/// 홈을 못 찾으면 (CI 등) 임시 경로로 fallback — save 가 안 되더라도 in-memory 동작.
fn file_handler_recent_path() -> std::path::PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handler-recent.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handler-recent.json"))
}

mod busy;
mod finders;
mod message;
mod pty;
mod terminal_finders;
