use std::collections::HashMap;
use std::sync::Arc;

use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::state::SurfaceMessage;
use crate::surface_registry::SurfaceKindRegistry;
use tasty_hooks::HookManager;
use tasty_terminal::Waker;

/// ID generator for workspaces, panes, tabs, and surfaces.
pub struct IdGenerator {
    workspace: u32,
    pane: u32,
    tab: u32,
    surface: u32,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            workspace: 1,
            pane: 1,
            tab: 1,
            surface: 1,
        }
    }

    pub fn next_workspace(&mut self) -> u32 {
        let id = self.workspace;
        self.workspace += 1;
        id
    }

    pub fn next_pane(&mut self) -> u32 {
        let id = self.pane;
        self.pane += 1;
        id
    }

    pub fn next_tab(&mut self) -> u32 {
        let id = self.tab;
        self.tab += 1;
        id
    }

    pub fn next_surface(&mut self) -> u32 {
        let id = self.surface;
        self.surface += 1;
        id
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
pub struct EngineState {
    // ── Workspace / Terminal management ──
    pub workspaces: Vec<Workspace>,
    pub next_ids: IdGenerator,
    pub default_cols: usize,
    pub default_rows: usize,
    pub waker: Waker,

    // ── Settings ──
    pub settings: Settings,

    // ── Notifications / Hooks ──
    pub notifications: NotificationStore,
    pub hook_manager: HookManager,
    pub global_hook_manager: GlobalHookManager,

    // ── Closed item history ──
    pub closed_items: crate::model::ClosedItemStore,

    // ── System clipboard history (memory-only) ──
    pub clipboard_history: crate::clipboard_history::ClipboardHistory,

    /// OSC 133 기반 명령 인덱서. PromptBoundary 이벤트가 도달할 때마다 호스트가
    /// 호출해 per-surface 상태를 업데이트하고, D phase 에서 memory 에 record 영속.
    pub command_index: crate::command_index::CommandIndex,

    /// 출력 옵저버 라우터. OutputAppended 이벤트마다 dispatch 호출.
    pub observer_router: crate::output_observer::ObserverRouter,

    /// 휴먼 핸드오프 — approval 요청/응답 큐 + 대기자 채널.
    pub approval_store: std::sync::Arc<tasty_approval::ApprovalStore>,

    /// Telemetry 이벤트 시퀀스 — 같은 ms 안에서 event_key 충돌 방지용 단조 증가 카운터.
    pub telemetry_seq: std::sync::Arc<tasty_telemetry::TelemetrySeq>,

    /// Telemetry 이상 탐지 — 호스트 singleton. in-memory sliding window 만 보관
    /// (Phase 4.4). 검출된 anomaly 레코드는 호스트가 memory store 에 영속.
    pub anomaly_detector: std::sync::Arc<tasty_telemetry::AnomalyDetector>,

    /// Agent task ID 시퀀스 — 같은 ms 안에서 task_id 충돌 방지용 단조 증가 카운터.
    pub agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,

    // ── Messaging / Typing detection ──
    pub surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub last_key_input: HashMap<u32, std::time::Instant>,

    // ── Busy state cache (foreground process != shell). Updated by BusyPoll.
    // Set membership = busy. Surfaces missing from the set are treated as idle.
    pub busy_surfaces: std::collections::HashSet<u32>,

    /// Targeted waker creation. winit `EventLoopProxy`를 직접 들지 않고 trait 뒤로
    /// 추상화하여 헤드리스/플러그인 호스트 컨텍스트에서도 동일 인터페이스를 쓴다.
    /// `App`이 EngineState 생성 후 본체에서 `WinitWakerFactory`를 주입한다.
    pub waker_factory: Option<tasty_core::SharedWakerFactory>,

    // ── CWD polling (round-robin) ──
    // macOS/Linux 전용. Windows에서는 폴링을 돌지 않아 필드 자체가 없음.
    // ── Surface kind registry ──
    /// Surface 종류별 메타·동작 lookup. 단계 03C에서는 빈 레지스트리만 보유한다 —
    /// 03D에서 본체 7종이 등록되며, 단계 05에서 plugin이 추가될 예정.
    pub surface_registry: Arc<SurfaceKindRegistry>,

    // ── File format / handler registries (file-handler-system) ──
    /// 파일 식별기 — host default + plugin contribute + user config 통합.
    /// `PluginManager` 와 같은 Arc 를 공유한다.
    pub file_format: Arc<crate::file_format::FileFormatRegistry>,
    /// 파일 핸들러 디스패치 테이블. `PluginManager` 와 같은 Arc 를 공유한다.
    pub file_handler: Arc<crate::file_handler::FileHandlerRegistry>,
    /// 사용자가 picker 에서 직접 고른 handler 의 LRU 기록 (보조 신호).
    /// 부팅 시 디스크에서 로드, 매 선택마다 atomic save.
    pub file_handler_recent: crate::file_handler_recent::RecentPicks,
    /// 비동기 파일 식별 worker. `App` 이 EventLoopProxy 를 가진 시점에
    /// `create_app_state` 에서 주입한다 — waker_factory 와 동일 패턴.
    /// Phase C 의 mouse.rs 콜사이트가 이걸 호출해 deep identify 를 띄운다.
    pub identify_worker: Option<std::sync::Arc<crate::identify_worker::IdentifyWorker>>,

    // ── Layout persistence ──
    pub layout_dirty: crate::layout_persistence::LayoutDirtyTracker,
    /// Active workspace index restored from layout.json. Consumed once by AppState::new().
    pub restored_active_workspace: Option<usize>,
    /// Deferred terminal surface 의 scrollback 복원 대기 큐. 값은
    /// `scrollback_store::read` 결과(없으면 entry 자체가 생략됨). PTY 가
    /// 실제로 spawn 된 직후 (`ensure_surface_initialized` 또는 즉시 복원
    /// 경로) entry 를 꺼내 `inject_scrollback` 호출.
    pub pending_scrollback_inject: HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,
    /// 첫 plugin pump 후 적용할 layout. plugin이 제공하는 surface kind가
    /// 등록되기 전에 복원하면 사라지므로 한 번 미뤄둔다. `App::apply_pending_layout_restore`가 소비.
    pub pending_layout_restore: Option<crate::layout_persistence::SavedLayout>,

    /// Whether input simulation IPC is enabled (debug builds only, --enable-input-simulation).
    #[cfg(debug_assertions)]
    pub input_simulation_enabled: bool,

    /// Layout preset 디스크 캐시 — App 의 `engine::Engine` 과 동일 Arc 공유.
    /// `create_app_state` 에서 주입한다.
    pub preset_store: Option<std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>>,
}

impl EngineState {
    /// Create a new EngineState with default settings.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let restore_layout = settings.general.restore_layout;

        // Create engine with empty workspaces first; we'll fill them below.
        let mut engine = Self {
            workspaces: Vec::new(),
            next_ids: IdGenerator::new(),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            closed_items: crate::model::ClosedItemStore::new(),
            clipboard_history: crate::clipboard_history::ClipboardHistory::new(100),
            command_index: crate::command_index::CommandIndex::new(),
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
                crate::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            file_format: {
                let reg = crate::file_format::FileFormatRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../../file_format/defaults/default-file-format.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler: {
                let reg = crate::file_handler::FileHandlerRegistry::new();
                reg.install_host_defaults(include_str!(
                    "../../file_handler/defaults/default-file-handlers.toml"
                ));
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler_recent: crate::file_handler_recent::RecentPicks::load(
                &file_handler_recent_path(),
            ),
            identify_worker: None,
            layout_dirty: crate::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            preset_store: None,
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
            if let Some(saved) = crate::layout_persistence::load_from_disk() {
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
                cols,
                rows,
                pane_id,
                tab_id,
                surface_id,
                sh.shell_ref(),
                &sh.args_ref(),
                waker,
                None,
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
        // 그 외에는 EngineState 생성 시 받은 base waker(`TerminalOutput(None)`)를 그대로 공유.
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
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            crate::surface_meta::SurfaceMetaStore::get(sid, "restore.command")
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
    pub fn record_file_handler_pick(&mut self, id: &crate::file_handler::HandlerId) {
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

/// `~/.tasty/file-handlers.toml` — 사용자 detector/handler 설정. 부팅 시 1회 로드.
fn file_handler_user_config_path() -> Option<std::path::PathBuf> {
    tasty_core::paths::tasty_home().map(|d| d.join("file-handlers.toml"))
}

/// `~/.tasty/file-handler-recent.json` — picker 선택 LRU. 부팅 시 로드, 매 선택마다 save.
/// 홈을 못 찾으면 (CI 등) 임시 경로로 fallback — save 가 안 되더라도 in-memory 동작.
fn file_handler_recent_path() -> std::path::PathBuf {
    tasty_core::paths::tasty_home()
        .map(|d| d.join("file-handler-recent.json"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handler-recent.json"))
}

mod pty;
mod terminal_finders;
