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
    /// Workspace category 카운터. `normal` 은 id 0 을 예약하므로 1 부터 발급.
    category: Arc<std::sync::atomic::AtomicU32>,
    pane: Arc<std::sync::atomic::AtomicU32>,
    tab: Arc<std::sync::atomic::AtomicU32>,
    surface: Arc<std::sync::atomic::AtomicU32>,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        use std::sync::atomic::AtomicU32;
        Self {
            workspace: Arc::new(AtomicU32::new(1)),
            category: Arc::new(AtomicU32::new(1)),
            pane: Arc::new(AtomicU32::new(1)),
            tab: Arc::new(AtomicU32::new(1)),
            surface: Arc::new(AtomicU32::new(1)),
        }
    }

    pub fn next_workspace(&self) -> u32 {
        self.workspace
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 새 workspace category id 발급(1 부터). `normal`(id 0)은 발급 대상 아님.
    pub fn next_category(&self) -> u32 {
        self.category
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// category 카운터를 `min_next` 이상으로 끌어올린다(이미 크면 no-op).
    /// 복원 시 layout.json 의 최대 카테고리 id + 1 위로 floor 를 올려 재사용 차단.
    pub fn bump_category_floor(&self, min_next: u32) {
        self.category
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
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

    /// surface 카운터를 `min_next` 이상으로 끌어올린다(이미 크면 no-op).
    /// 다음 `next_surface()` 가 반환할 값이 `>= min_next` 가 되도록 보장.
    ///
    /// 재시작 시 `surface_id` 는 매 실행 1 부터 재발급되는데 surface_meta
    /// (`memory.db`)는 영속되므로, 복원이 발급하는 id 가 이전 실행의 stale
    /// `Scope::Surface(id)` 와 겹칠 수 있다. 복원 *직전* 에 floor 를 stale 최대
    /// id 위로 올려 재사용을 원천 차단한다.
    pub fn bump_surface_floor(&self, min_next: u32) {
        self.surface
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
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

/// Explorer 파일 클립보드 (T11) — 복사/잘라내기한 경로 + cut 여부.
#[derive(Clone, Debug)]
pub struct ExplorerClipboard {
    pub paths: Vec<std::path::PathBuf>,
    /// true = 잘라내기(이동), false = 복사.
    pub cut: bool,
}

/// N-RA02 — 원격 워크스페이스 추가 팝업의 Connect 가 메인 루프로 넘기는 사용자-경로
/// GUI attach 요청. `port`/`workspace` 는 attach 대상, `tunnel` 은 조회에 쓴 SSH 터널을
/// 재사용(loopback 이면 None). `App::dispatch_pending_gui_attach` 가 drain 해
/// `start_gui_attach` 로 mirror 를 띄우고, 성공 시 새 mirror ws 로 focus 를 옮긴다.
/// `Clone`/`Debug` 불가(SshTunnel = 자식 process 핸들) — 큐로 단발 이동한다.
pub(crate) struct GuiAttachUserReq {
    pub(crate) port: u16,
    pub(crate) workspace: u32,
    pub(crate) tunnel: Option<tasty_cli::ssh::SshTunnel>,
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
    /// Workspace category(사이드바 폴더) 목록. Vec 순서 = 사이드바 섹션 표시 순서.
    /// `categories[0]` 은 항상 예약된 `normal`(id 0, 위치 고정). [`CoreState::ensure_normal_category`]
    /// 가 생성/복원 직후 이 불변식을 보장한다.
    pub(crate) categories: Vec<crate::model::WorkspaceCategory>,
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

    /// `agent.task_await` blocking 용 waker hub. set_state 가 종결 전이 시 fire,
    /// task_await 가 등록 후 recv_timeout.
    pub(crate) task_waker_hub: std::sync::Arc<crate::core::agent::task_waker::TaskWakerHub>,

    // ── Messaging / Typing detection ──
    pub(crate) surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub(crate) last_key_input: HashMap<u32, std::time::Instant>,

    // ── Busy state cache (foreground process != shell). Updated by BusyPoll.
    // Set membership = busy. Surfaces missing from the set are treated as idle.
    pub(crate) busy_surfaces: std::collections::HashSet<u32>,

    // ── Mouse-capture blacklist cache. Updated by the same 1Hz BusyPoll using
    // the foreground names already resolved for busy detection (no extra
    // process snapshot). Set membership = that surface's foreground process
    // matches `mouse_capture_blacklist`, so its click/drag capture is disabled.
    pub(crate) mouse_capture_disabled_surfaces: std::collections::HashSet<u32>,

    // ── Foreground process-name cache (surface_id → display name). Updated by
    // the same 1Hz BusyPoll from the foreground programs it already resolves
    // (no extra process snapshot). The StatusBar reads this every frame instead
    // of re-snapshotting all system processes per frame. Replaced wholesale each
    // tick so names of closed surfaces never linger.
    pub(crate) foreground_names: std::collections::HashMap<u32, String>,

    /// Surface *cut/move* slot (T9). 사용자가 우클릭 컨텍스트 메뉴에서 "잘라내기"
    /// 한 surface 의 id 를 들고 있다가, 다른 위치에서 "여기로 이동" 하면 그 surface 를
    /// 살아있는 채로 이동(replace)한다. 단일 슬롯·세션 휘발(스냅샷 아님, layout.json
    /// 영속 대상 아님). set/clear 는 사용자 우클릭 조작이라 release 경로에서 직접 갱신
    /// 한다(도메인 mutate 아님).
    pub(crate) pending_move_surface: Option<crate::model::SurfaceId>,

    /// Explorer 파일 클립보드 (T11). 우클릭 "복사"/"잘라내기"한 경로 집합 +
    /// cut 여부를 들고 있다가 "붙여넣기"에서 소비한다. OS 텍스트 클립보드와 별개의
    /// explorer 내부 파일 이동 슬롯 — 단일 슬롯·세션 휘발(layout.json 비영속).
    /// 사용자 우클릭 조작이라 release 경로에서 직접 갱신(도메인 mutate 아님).
    pub(crate) explorer_clipboard: Option<ExplorerClipboard>,

    /// Explorer 즐겨찾기 (T11). 전역(surface 무관)·영속 — 부팅 시
    /// `ExplorerFavorites::load()` 로 `~/.tasty/explorer-favorites.toml` 에서 읽고,
    /// 우클릭 추가/제거 시 메모리 갱신 + `save()` 로 즉시 디스크 반영한다. 사용자
    /// 직접 조작으로만 변경되므로 release 경로에서 직접 갱신(도메인 snapshot 비대상).
    /// (소비자가 전부 gui 어댑터라 headless 빌드에선 필드째 제외.)
    #[cfg(feature = "gui")]
    pub(crate) explorer_favorites: crate::explorer_ui::favorites::ExplorerFavorites,

    /// **Phase D D.3.E.4** — Terminal/PTY 데이터 owner (Surface 트리와 분리).
    /// 신설 단계 (E.4.a) 에서는 *빈 store* 만 보유, 호출처 0. 후속 E.4.b ~ f 에서
    /// 점진적으로 *Terminal 인스턴스 / deferred / scrollback_persist / pending
    /// scrollback inject / busy_surfaces* 가 이쪽으로 이전된다. cutover (E.4.f)
    /// 시 위 `busy_surfaces` 필드는 store 안 동일 이름 필드로 통합 폐기 예정.
    pub(crate) terminals: crate::core::terminal_store::TerminalStore,

    /// 배타적 attach 점유 lock (attach/detach 단계 3). surface_id → 점유 client.
    /// 휘발성 — 직렬화/복원 안 함(decision 2). client_id 는 단계 1 StreamClientId.
    pub(crate) attach: crate::core::attach::AttachRegistry,

    /// attach/detach 작업 J — 서버측 readonly 뷰의 display-only mirror.
    /// 점유된 surface 마다 detached `Terminal`(grid 표시 전용)을 두고, 3초 `AttachPoll`
    /// tick 때 live grid 스냅샷을 feed 한다(plan §2.3). render_pass 가 is_attached
    /// surface 를 이 mirror 로 렌더해 "내용 보임 + 조작만 차단 + 3초 cadence" readonly
    /// 를 구현한다. live Terminal 은 PTY 소유·입력 차단 전용으로 유지. 휘발성.
    /// headless 는 렌더가 없어 읽지 않는다(gui 한정).
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) readonly_views: HashMap<u32, tasty_terminal::Terminal>,

    /// attach/detach 작업 J — GUI in-process attach-client 트리거 큐. IPC
    /// `attach.into_gui {port, workspace}` 핸들러가 `(port, workspace)` 를 push 하면
    /// App 이 `about_to_wait` 에서 drain 해 원격 워크스페이스를 mirror 로 재구성한다
    /// (focus 비의존, plan §5). headless 는 GUI 가 없어 drain 되지 않는다.
    pub(crate) pending_gui_attach: Vec<(u16, u32)>,

    /// N-RA02 — **사용자 입력 경로 전용** GUI attach 트리거 큐. 원격 워크스페이스 추가
    /// 팝업(remote_attach)의 Connect 클릭이 조회에 쓴 터널을 실어 push 한다. 위
    /// `pending_gui_attach`(IPC/에이전트 경로, focus 중립)와 분리된 이유: 이 큐 drain 은
    /// attach 성공 시 새 mirror ws 로 **focus 를 이동**하는데(사용자 확정 동작), 그 focus
    /// 이동은 사용자 입력 경로에서만 허용된다(원칙 1②). release IPC/CLI 는 이 큐에 push
    /// 하지 않는다.
    pub(crate) pending_gui_attach_user: Vec<GuiAttachUserReq>,

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

    /// Plugin 이 manifest `[[contributes.hook_events]]` 로 선언한 surface hook
    /// 이벤트 키 집계. plugin hello 시 등록, unload/remove 시 제거. `hook.set` /
    /// `surface.fire_hook` 핸들러가 (내장 ∪ 활성 plugin 선언) 검증에 사용한다.
    pub(crate) plugin_hook_events: Arc<crate::engine::hook_event_registry::PluginHookEventRegistry>,

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
    #[cfg(feature = "gui")]
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
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) input_simulation_enabled: bool,

    /// Memory port 의 Arc clone — Core 가 owner. 생성자에서 즉시 주입되며
    /// engine 내부 (SurfaceMetaStore, layout persistence, pty surface init 등)
    /// cascade 없이 직접 영속할 때 사용.
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
}

impl CoreState {
    /// Create a new CoreState with default settings.
    ///
    /// 테스트 / non-host 진입점용 변형. 내부에서 in-memory `MemoryStore` 를
    /// 생성해 주입한다. host 부팅 경로는 `new_with_ids` 를 사용한다.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory()?,
            ));
        Self::new_with_ids(cols, rows, waker, None, memory)
    }

    /// 새 CoreState 를 만들 때 기존 ID 공간(Arc<AtomicU32> 들)을 공유받는 변형.
    /// multi-window 시 두 번째 main 의 첫 workspace 가 첫 engine 과 ID 충돌하지
    /// 않게 한다. `shared_ids=None` 이면 새 IdGenerator 로 1부터 시작.
    pub fn new_with_ids(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let restore_layout = settings.general.restore_layout;

        // Create engine with empty workspaces first; we'll fill them below.
        let mut engine = Self {
            workspaces: Vec::new(),
            categories: vec![crate::model::WorkspaceCategory::normal()],
            next_ids: shared_ids.unwrap_or_default(),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            closed_items: crate::model::ClosedItemStore::new(),
            command_index: crate::engine::command_index::CommandIndex::new(),
            observer_router: crate::output_observer::ObserverRouter::new(),
            approval_store: std::sync::Arc::new(tasty_approval::ApprovalStore::new()),
            telemetry_seq: std::sync::Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: std::sync::Arc::new(tasty_telemetry::AnomalyDetector::new()),
            agent_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            task_waker_hub: std::sync::Arc::new(crate::core::agent::task_waker::TaskWakerHub::new()),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            busy_surfaces: std::collections::HashSet::new(),
            mouse_capture_disabled_surfaces: std::collections::HashSet::new(),
            foreground_names: std::collections::HashMap::new(),
            pending_move_surface: None,
            explorer_clipboard: None,
            #[cfg(feature = "gui")]
            explorer_favorites: crate::explorer_ui::favorites::ExplorerFavorites::load(),
            terminals: crate::core::terminal_store::TerminalStore::new(),
            attach: crate::core::attach::AttachRegistry::new(),
            readonly_views: HashMap::new(),
            pending_gui_attach: Vec::new(),
            pending_gui_attach_user: Vec::new(),
            waker_factory: None,
            surface_registry: {
                let reg = SurfaceKindRegistry::new();
                crate::engine::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            plugin_hook_events: Arc::new(
                crate::engine::hook_event_registry::PluginHookEventRegistry::new(),
            ),
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
            #[cfg(feature = "gui")]
            identify_worker: None,
            layout_dirty: crate::engine::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            memory,
        };

        // (Phase E) FileHandler 가 detector 메타 (광고 확장자 등) 를 조회할 수 있게
        // FileFormatRegistry 를 DetectorInfo 로 주입. host default 가 이미 로드된 시점.
        engine
            .file_handler
            .attach_detector_info(engine.file_format.clone());

        // Re-apply coalesce_ms from actual settings
        engine.notifications =
            NotificationStore::with_coalesce_ms(engine.settings.notification.coalesce_ms);

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
            let terminal = crate::model::Pane::spawn_terminal(
                surface_id,
                crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker,
                    working_dir: None,
                },
            )?;
            engine.terminals.insert(surface_id, terminal);
            let ws = Workspace::new_with_terminal_marker(
                ws_id,
                "Workspace 1".to_string(),
                pane_id,
                tab_id,
                surface_id,
            );
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
        if self.settings.performance.targeted_pty_polling
            && let Some(factory) = &self.waker_factory
        {
            return factory.make_targeted_waker(surface_id);
        }
        self.waker.clone()
    }

    /// Push a closed item, automatically injecting restore commands from surface metadata.
    /// Plugins write the `restore.command` meta key directly (host stays agent-agnostic).
    pub fn push_closed_item(&mut self, mut item: crate::model::ClosedItem) {
        let mem = self.memory.clone();
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            let mut guard = match mem.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            crate::surface_meta::SurfaceMetaStore::get(&mut *guard, sid, "restore.command")
        });
        // Persist the captured scrollback to disk so the retained closed item
        // holds only a reference (persist_id), not up to 10k lines per surface.
        // A fresh id is used (not the live surface's layout persist_id, which
        // `cleanup_surface` deletes on close), so the two never collide. Stale
        // closed-item files are reclaimed by `gc_orphans` on the next startup,
        // since closed items do not survive a restart.
        crate::model::closed_item::persist_closed_scrollback(&mut item, &mut |lines| {
            let id = crate::scrollback_store::new_persist_id();
            match crate::scrollback_store::write(&id, lines) {
                Ok(()) => Some(id),
                Err(e) => {
                    tracing::warn!("closed-item scrollback persist failed: {e}");
                    None
                }
            }
        });
        // Evicting the oldest item must release its backing scrollback files,
        // otherwise `~/.tasty/scrollback/*.bin` orphans accumulate for the rest
        // of the session.
        if let Some(evicted) = self.closed_items.push(item) {
            let mut refs = Vec::new();
            crate::model::closed_item::collect_scrollback_refs(&evicted, &mut refs);
            for id in refs {
                crate::scrollback_store::delete(&id);
            }
        }
    }

    /// Record that the user typed on the given surface.
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    /// Re-plumb the current global theme palette into every terminal so OSC
    /// 10/11/12/4 color queries report the new theme. Called on theme change.
    pub fn resync_terminal_palettes(&mut self) {
        self.terminals.resync_palettes();
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
    /// SurfaceKindRegistry를 통해 새 surface 인스턴스를 만든다.
    /// `"terminal"`은 호출자가 PTY spawn 경로로 분기 처리해야 하므로 여기서는 처리하지 않는다.
    ///
    /// AppState 가 아닌 CoreState 의 메서드 — surface_registry 는 engine 의 일.
    /// D.3.C.B.1 step 1 에서 AppState::create_surface_via_registry 를 옮김.
    ///
    /// `cwd` 는 *carry cwd* — 호출자(intent / preset / convert)가 source surface 의
    /// source_cwd 를 resolve 해 명시 전달한다. surface kind 가 사용 여부를 결정.
    /// Surface cwd invariant — `docs/architecture/invariants/surface-cwd.md` 참조.
    pub(crate) fn create_surface_via_registry(
        &self,
        kind: &str,
        surface_id: u32,
        cwd: Option<&std::path::Path>,
        params: &serde_json::Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        let def = self
            .surface_registry
            .get(kind)
            .ok_or_else(|| anyhow::anyhow!("unknown surface kind: {}", kind))?;
        // 새 explorer 는 "마지막으로 고른 view mode"(Settings)로 열린다. params 가
        // view_mode 를 명시하지 않은 경우에만 주입한다(명시 우선). restore 경로는
        // create 를 거치지 않으므로 per-tab 저장값이 그대로 유지된다.
        if kind == "explorer"
            && params.get("view_mode").is_none()
            && let serde_json::Value::Object(mut map) = params.clone()
        {
            map.insert(
                "view_mode".to_string(),
                serde_json::Value::String(self.settings.general.explorer_view_mode.clone()),
            );
            return (def.create)(surface_id, cwd, &serde_json::Value::Object(map));
        }
        (def.create)(surface_id, cwd, params)
    }
}

impl CoreState {
    /// Refresh the cached display name of the tab containing a given surface ID.
    pub fn refresh_tab_display_name(&mut self, surface_id: u32) {
        let workspaces = &mut self.workspaces;
        let terminals = &self.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            let cwd = terminals.get(tab.focused_surface).and_then(|t| t.get_cwd());
                            tab.refresh_display_name(cwd.as_deref());
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Re-project the OSC title of the tab containing `surface_id` from that
    /// tab's *focused* surface only. Mirror of `refresh_tab_display_name` for the
    /// OSC-title path — keeps both "focused-surface projection" policies aligned.
    ///
    /// `surface_id` need only be *some* surface of the tab (not necessarily the
    /// focused one) — the tab is located by membership, then its
    /// `focused_surface`'s current title is read. When the focused surface has no
    /// title (non-terminal, or a terminal that never emitted OSC 0/2), `osc_title`
    /// is cleared so `display_name()` falls back to the cwd-derived name → auto
    /// name. `explicit_name` tabs are left untouched.
    pub fn refresh_tab_osc_title(&mut self, surface_id: u32) {
        let workspaces = &mut self.workspaces;
        let terminals = &self.terminals;
        for workspace in workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pid) {
                    for tab in &mut pane.tabs {
                        if tab.contains_surface(surface_id) {
                            if tab.explicit_name.is_some() {
                                return;
                            }
                            tab.osc_title = terminals
                                .get(tab.focused_surface)
                                .and_then(|t| t.current_title());
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

// ── Workspace category 관리 ──
impl CoreState {
    /// normal 불변식 보장 — `categories[0]` 이 항상 예약된 normal 이 되도록 정규화.
    ///
    /// 생성/복원 직후 호출한다. (1) normal 이 없으면 맨 앞에 삽입, (2) 있으나 0번이
    /// 아니면 0번으로 이동, (3) 또한 발급기 floor 를 기존 최대 카테고리 id + 1 이상으로
    /// 올려 재사용을 차단한다. 어떤 워크스페이스가 존재하지 않는 카테고리를 가리키면
    /// normal 로 귀속한다.
    pub fn ensure_normal_category(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        // (1)(2) normal 을 0번에 고정.
        match self.categories.iter().position(|c| c.is_normal()) {
            Some(0) => {}
            Some(idx) => {
                let normal = self.categories.remove(idx);
                self.categories.insert(0, normal);
            }
            None => self.categories.insert(0, WorkspaceCategory::normal()),
        }
        // (3) 발급기 floor 를 최대 사용자 카테고리 id + 1 위로.
        let max_id = self
            .categories
            .iter()
            .map(|c| c.id)
            .filter(|&id| id != NORMAL_CATEGORY_ID)
            .max();
        if let Some(max_id) = max_id {
            self.next_ids.bump_category_floor(max_id + 1);
        }
        // 존재하지 않는 카테고리를 가리키는 워크스페이스는 normal 로 귀속.
        let valid: std::collections::HashSet<u32> = self.categories.iter().map(|c| c.id).collect();
        for ws in &mut self.workspaces {
            if !valid.contains(&ws.category) {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
    }

    /// 카테고리 목록(읽기 전용).
    pub fn categories(&self) -> &[crate::model::WorkspaceCategory] {
        &self.categories
    }

    /// 카테고리 토글 **off** 마이그레이션 (§4-2 on→off). normal 외 모든 카테고리를
    /// 제거하고 그 안의 워크스페이스를 모두 normal 로 귀속한다. **워크스페이스의 물리
    /// 순서(전역 인덱스)는 그대로 두므로** active(전역 인덱스) 도 불변이다 — 사이드바가
    /// off 면 평면이라 순서만 보존되면 충분하다.
    pub fn collapse_categories_to_normal(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        for ws in &mut self.workspaces {
            ws.set_category(NORMAL_CATEGORY_ID);
        }
        self.categories = vec![WorkspaceCategory::normal()];
    }

    /// 새 카테고리 생성. 이름 검증(§3, 대소문자 무시 중복·예약어 거부) 후 새 id 를
    /// 발급해 Vec 끝(normal 뒤)에 push 한다. 성공 시 새 카테고리 id 반환.
    ///
    /// 카테고리 CRUD 는 **사용자 active/포커스에 닿지 않는** 순수 도메인 데이터 변경이라
    /// (원칙 1·3) cascade/active 보정이 필요 없다 — `set_attach_mapping` 직접 set +
    /// mark_layout_dirty 선례와 동형. 호출자가 mark_layout_dirty 를 책임진다.
    pub fn create_category(
        &mut self,
        raw_name: &str,
    ) -> Result<crate::model::WorkspaceCategoryId, crate::model::CategoryNameError> {
        let existing: Vec<&str> = self.categories.iter().map(|c| c.name.as_str()).collect();
        let name = crate::model::validate_new_category_name(raw_name, existing)?;
        let id = self.next_ids.next_category();
        self.categories
            .push(crate::model::WorkspaceCategory::new(id, name));
        Ok(id)
    }

    /// 카테고리 이름 변경. normal 은 거부(`IsNormal`), 이름 검증은 대상 자신을 제외한
    /// 나머지와 비교한다.
    pub fn rename_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        raw_name: &str,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        if self.category_index(id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let existing: Vec<&str> = self
            .categories
            .iter()
            .filter(|c| c.id != id)
            .map(|c| c.name.as_str())
            .collect();
        let name = crate::model::validate_rename_category_name(raw_name, existing)
            .map_err(CategoryOpError::Name)?;
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.name = name;
        }
        Ok(())
    }

    /// 카테고리 삭제. normal 은 거부. 삭제 대상 안의 워크스페이스는 **순서를 보존하며**
    /// normal 로 귀속한다(전역 인덱스 불변 → 사용자 active 영향 없음, 원칙 1·3).
    pub fn delete_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        let idx = self.category_index(id).ok_or(CategoryOpError::NotFound)?;
        for ws in &mut self.workspaces {
            if ws.category == id {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
        self.categories.remove(idx);
        Ok(())
    }

    /// 카테고리 순서 이동(reorder). **from==0 또는 to==0 거부**(normal 0번 고정).
    /// 범위 밖이거나 from==to 면 no-op(false).
    pub fn reorder_category(
        &mut self,
        from_index: usize,
        to_index: usize,
    ) -> Result<(), CategoryOpError> {
        let len = self.categories.len();
        if from_index == 0 || to_index == 0 {
            return Err(CategoryOpError::NormalFixed);
        }
        if from_index >= len || to_index >= len {
            return Err(CategoryOpError::InvalidIndex);
        }
        if from_index == to_index {
            return Ok(());
        }
        let cat = self.categories.remove(from_index);
        self.categories.insert(to_index, cat);
        Ok(())
    }

    /// 워크스페이스의 카테고리 소속 변경. 대상 카테고리가 존재해야 한다. **사용자
    /// active(전역 인덱스) 불변** — 소속만 바꾼다(원칙 1·3).
    pub fn set_workspace_category(
        &mut self,
        ws_id: crate::model::WorkspaceId,
        cat_id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        if self.category_index(cat_id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws_id)
            .ok_or(CategoryOpError::WorkspaceNotFound)?;
        ws.set_category(cat_id);
        Ok(())
    }

    /// 카테고리 접힘(collapsed) 상태 설정. id 로 카테고리를 찾아 `collapsed` 를
    /// 지정 값으로 둔다. normal 포함 모든 카테고리 접기 허용(디자인상 normal 도 접힘 가능).
    /// 접힘은 사용자 UI 상태지만 layout.json 영속 대상이므로 호출자가 다른 mutator
    /// 관례대로 `mark_layout_dirty` 를 책임진다. 대상이 없으면 no-op.
    pub fn set_category_collapsed(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        collapsed: bool,
    ) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = collapsed;
        }
    }

    /// 카테고리 접힘 상태를 뒤집는다(호출부 단순화용 편의 메서드). 대상이 없으면 no-op.
    pub fn toggle_category_collapsed(&mut self, id: crate::model::WorkspaceCategoryId) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = !cat.collapsed;
        }
    }

    /// 이름(대소문자 무시) 또는 정확 id 로 카테고리를 해석한다. CLI/IPC 가
    /// `--category <name|id>` 를 받을 때 사용. 숫자 문자열은 id 로 우선 해석한다.
    pub fn resolve_category(&self, token: &str) -> Option<crate::model::WorkspaceCategoryId> {
        let t = token.trim();
        if let Ok(id) = t.parse::<u32>()
            && self.categories.iter().any(|c| c.id == id)
        {
            return Some(id);
        }
        self.categories
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(t))
            .map(|c| c.id)
    }

    /// 카테고리 id → 이름. 없으면 None.
    pub fn category_name(&self, id: crate::model::WorkspaceCategoryId) -> Option<&str> {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }
}

/// 카테고리 변경 연산 에러. IPC 핸들러가 사용자 메시지로 매핑한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryOpError {
    /// 대상 카테고리를 찾을 수 없음.
    NotFound,
    /// normal 은 rename/delete 불가.
    IsNormal,
    /// normal(0번) 위치 고정 위반(reorder from/to == 0).
    NormalFixed,
    /// reorder 인덱스 범위 밖.
    InvalidIndex,
    /// 워크스페이스를 찾을 수 없음(set_workspace_category).
    WorkspaceNotFound,
    /// 이름 검증 실패.
    Name(crate::model::CategoryNameError),
}

impl std::fmt::Display for CategoryOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CategoryOpError::NotFound => f.write_str("category not found"),
            CategoryOpError::IsNormal => {
                f.write_str("the 'normal' category cannot be renamed or deleted")
            }
            CategoryOpError::NormalFixed => {
                f.write_str("the 'normal' category is fixed at position 0")
            }
            CategoryOpError::InvalidIndex => f.write_str("category index out of range"),
            CategoryOpError::WorkspaceNotFound => f.write_str("workspace not found"),
            CategoryOpError::Name(e) => write!(f, "{e}"),
        }
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

// 유일한 소비자가 gui 전용 port_scanner popup 이라 headless 에서는 unused.
#[cfg(feature = "gui")]
pub use finders::SurfaceDisplayPath;

#[cfg(test)]
mod id_generator_tests {
    use super::IdGenerator;

    #[test]
    fn next_surface_starts_at_one() {
        let ids = IdGenerator::new();
        assert_eq!(ids.next_surface(), 1);
        assert_eq!(ids.next_surface(), 2);
    }

    #[test]
    fn bump_surface_floor_raises_counter() {
        let ids = IdGenerator::new();
        ids.bump_surface_floor(18);
        assert_eq!(
            ids.next_surface(),
            18,
            "floor 이후 첫 id 는 min_next 와 같아야 한다"
        );
        assert_eq!(ids.next_surface(), 19);
    }

    #[test]
    fn bump_surface_floor_is_noop_when_already_higher() {
        let ids = IdGenerator::new();
        // 카운터를 5 까지 소비 (1..=4 발급, 다음은 5).
        for _ in 0..4 {
            ids.next_surface();
        }
        ids.bump_surface_floor(3); // 현재 floor(5)보다 낮음 → 무시.
        assert_eq!(ids.next_surface(), 5);
    }
}

#[cfg(test)]
mod category_tests {
    use super::{CategoryOpError, CoreState};
    use crate::model::{CategoryNameError, NORMAL_CATEGORY_ID};

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn starts_with_normal_only() {
        let e = engine();
        assert_eq!(e.categories().len(), 1);
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
        assert!(e.categories()[0].is_normal());
    }

    #[test]
    fn create_rejects_reserved_and_duplicate() {
        let mut e = engine();
        let id = e.create_category("Work").unwrap();
        assert_ne!(id, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 2);
        assert_eq!(
            e.create_category("normal"),
            Err(CategoryNameError::Reserved)
        );
        assert_eq!(e.create_category("WORK"), Err(CategoryNameError::Duplicate));
    }

    #[test]
    fn rename_rejects_normal() {
        let mut e = engine();
        assert_eq!(
            e.rename_category(NORMAL_CATEGORY_ID, "x"),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn delete_moves_workspaces_to_normal() {
        let mut e = engine();
        let cat = e.create_category("Work").unwrap();
        // 기본 워크스페이스를 새 카테고리로 옮긴다.
        let ws_id = e.workspaces[0].id;
        e.set_workspace_category(ws_id, cat).unwrap();
        assert_eq!(e.workspaces[0].category, cat);
        // 삭제 → 워크스페이스는 normal 로 귀속, 카테고리 제거.
        e.delete_category(cat).unwrap();
        assert_eq!(e.workspaces[0].category, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 1);
        // normal 삭제는 거부.
        assert_eq!(
            e.delete_category(NORMAL_CATEGORY_ID),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn reorder_protects_normal_position() {
        let mut e = engine();
        e.create_category("a").unwrap();
        e.create_category("b").unwrap();
        // from/to == 0 거부.
        assert_eq!(e.reorder_category(0, 1), Err(CategoryOpError::NormalFixed));
        assert_eq!(e.reorder_category(1, 0), Err(CategoryOpError::NormalFixed));
        // 1↔2 스왑은 허용.
        assert!(e.reorder_category(1, 2).is_ok());
        // normal 은 여전히 0번.
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
    }

    #[test]
    fn resolve_category_by_name_or_id() {
        let mut e = engine();
        let id = e.create_category("Study").unwrap();
        assert_eq!(e.resolve_category("study"), Some(id));
        assert_eq!(e.resolve_category(&id.to_string()), Some(id));
        assert_eq!(e.resolve_category("nope"), None);
    }

    #[test]
    fn set_collapsed_updates_memory() {
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        // 기본은 펼침.
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, false);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        // 없는 id 는 no-op(패닉 없이 무시).
        e.set_category_collapsed(9999, true);
    }

    #[test]
    fn toggle_collapsed_flips() {
        let mut e = engine();
        let id = e.create_category("Toggle").unwrap();
        e.toggle_category_collapsed(id);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.toggle_category_collapsed(id);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn set_collapsed_allows_normal() {
        // 디자인상 normal 도 접힘 가능.
        let mut e = engine();
        e.set_category_collapsed(NORMAL_CATEGORY_ID, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == NORMAL_CATEGORY_ID)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn collapsed_survives_layout_round_trip() {
        use crate::engine::layout_persistence::SavedLayout;
        // 접힘 상태를 설정한 뒤 layout capture→restore 왕복 후에도 유지되는지 회귀.
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        e.set_category_collapsed(id, true);
        let saved = SavedLayout::capture(&mut e, 0);

        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        let cat = restored
            .categories()
            .iter()
            .find(|c| c.name == "Services")
            .expect("Services category should be restored");
        assert!(
            cat.collapsed,
            "collapsed 상태가 왕복 후에도 유지되어야 한다"
        );
    }

    #[test]
    fn mirror_workspace_not_persisted() {
        use crate::engine::layout_persistence::SavedLayout;
        // N-RA02 회귀: 원격 attach 가 만드는 mirror workspace 는 layout.json 에 저장되면
        // 안 된다(재시작 시 원격 없는 죽은 일반 ws 로 복원되는 버그). capture 가 제외하고
        // active 인덱스도 필터 후 위치로 remap 하는지 확인.
        let mut e = engine();
        let base_count = e.workspaces.len();
        assert!(base_count >= 1, "엔진은 기본 workspace 를 하나 이상 가진다");
        // 원격 attach 가 만드는 mirror workspace 를 흉내 — 새 ws 를 만들고 mirror 플래그를 세운다.
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            None,
            "terminal".to_string(),
            serde_json::Value::Null,
            None,
            None,
            None,
            None,
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        e.workspaces[idx].mirror = true;
        assert_eq!(e.workspaces.len(), base_count + 1);

        // active 를 mirror 로 둔 채 capture — mirror 는 저장 제외 + active 는 클램프.
        let saved = SavedLayout::capture(&mut e, idx);
        assert_eq!(
            saved.workspaces.len(),
            base_count,
            "mirror workspace 는 저장에서 제외돼야 한다"
        );
        assert!(
            saved.active_workspace < saved.workspaces.len(),
            "remap 된 active 인덱스가 저장 목록 범위 안이어야 한다"
        );

        // 왕복 복원 후에도 mirror 는 되살아나지 않는다.
        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        assert!(
            restored.workspaces.iter().all(|w| !w.mirror),
            "복원본에 mirror workspace 가 없어야 한다"
        );
    }

    #[test]
    fn create_workspace_inner_assigns_category() {
        // 생성 시점 카테고리 소속(옵션 A). 유효 카테고리는 그 소속, dangling 은 normal.
        let mut e = engine();
        let cat = e.create_category("Services").unwrap();
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            None,
            "terminal".to_string(),
            serde_json::Value::Null,
            None,
            None,
            None,
            Some(cat),
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        assert_eq!(e.workspaces[idx].category, cat);

        // 존재하지 않는 카테고리 → normal 유지.
        let idx2 = match crate::core::apply_create_workspace_inner(
            &mut e,
            None,
            "terminal".to_string(),
            serde_json::Value::Null,
            None,
            None,
            None,
            Some(9999),
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => unreachable!(),
        };
        assert_eq!(e.workspaces[idx2].category, NORMAL_CATEGORY_ID);
    }
}
