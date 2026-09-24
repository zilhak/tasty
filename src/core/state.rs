use std::collections::HashMap;
use std::sync::Arc;

use crate::core::surface_registry::SurfaceKindRegistry;
use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
pub(crate) use message::SurfaceMessage;
use tasty_hooks::HookManager;
use tasty_terminal::Waker;

/// 여러 engine이 같은 Arc 카운터를 써 ID가 겹치지 않게 한다. Clone도 카운터를 공유한다.
/// u32·u64 카운터의 overflow나 ID 범위 소진을 여기서 별도로 막지는 않는다.
#[derive(Clone)]
pub struct IdGenerator {
    workspace: Arc<std::sync::atomic::AtomicU32>,
    /// normal 카테고리의 0을 예약하고 1에서 시작한다.
    category: Arc<std::sync::atomic::AtomicU32>,
    pane: Arc<std::sync::atomic::AtomicU32>,
    tab: Arc<std::sync::atomic::AtomicU32>,
    surface: Arc<std::sync::atomic::AtomicU32>,
    /// 같은 TerminalStore에 넣는 PTY ID는 PTY_ID_BASE에서 시작한다.
    pty: Arc<std::sync::atomic::AtomicU32>,
    observer: Arc<std::sync::atomic::AtomicU64>,
    hook: Arc<std::sync::atomic::AtomicU64>,
    global_hook: Arc<std::sync::atomic::AtomicU32>,
    /// 알림 저장소는 engine별이며 ID·생성 순번은 프로세스에서 공유한다.
    notification: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        Self {
            workspace: Arc::new(AtomicU32::new(1)),
            category: Arc::new(AtomicU32::new(1)),
            pane: Arc::new(AtomicU32::new(1)),
            tab: Arc::new(AtomicU32::new(1)),
            surface: Arc::new(AtomicU32::new(1)),
            pty: Arc::new(AtomicU32::new(crate::core::pty_registry::PTY_ID_BASE)),
            observer: Arc::new(AtomicU64::new(1)),
            hook: Arc::new(AtomicU64::new(1)),
            global_hook: Arc::new(AtomicU32::new(0)),
            notification: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn pty_counter(&self) -> Arc<std::sync::atomic::AtomicU32> {
        Arc::clone(&self.pty)
    }

    pub fn observer_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.observer)
    }

    pub fn hook_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.hook)
    }

    pub fn global_hook_counter(&self) -> Arc<std::sync::atomic::AtomicU32> {
        Arc::clone(&self.global_hook)
    }

    pub fn notification_counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.notification)
    }

    pub fn next_workspace(&self) -> u32 {
        self.workspace
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn next_category(&self) -> u32 {
        self.category
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 복원한 카테고리 ID를 재사용하지 않도록 다음 발급 기준을 높인다. 이미 더 크면 유지한다.
    #[cfg(any(feature = "gui", test))]
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

    /// 이전 실행의 surface 메타데이터 ID를 피하도록 다음 발급 기준을 높인다.
    /// 현재 기준을 낮추지 않으며 이후 overflow까지 막는 함수는 아니다.
    #[cfg(any(feature = "gui", test))]
    pub fn bump_surface_floor(&self, min_next: u32) {
        self.surface
            .fetch_max(min_next, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
    /// 셸 초기화에 필요한 추가 환경변수. bash의 rcfile 설정은 args로 전달한다.
    pub envs: Vec<(String, String)>,
}

impl ShellConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            shell: settings.general.shell.clone(),
            args: settings.general.effective_shell_args(),
            envs: settings.general.effective_shell_envs(),
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

    pub fn envs_ref(&self) -> Vec<(&str, &str)> {
        self.envs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

#[derive(Clone, Debug)]
#[cfg(feature = "gui")]
pub struct ExplorerClipboard {
    pub paths: Vec<std::path::PathBuf>,
    pub cut: bool,
}

/// 사용자가 원격 연결 팝업에서 확정한 요청. 조회에 쓴 SSH 터널을 함께 넘길 수 있다.
/// IPC 요청과 달리 연결 성공 후 새 mirror를 선택할 수 있어 별도 큐다.
#[cfg(feature = "gui")]
pub(crate) struct GuiAttachUserReq {
    pub(crate) port: u16,
    pub(crate) workspace: u32,
    pub(crate) tunnel: Option<tasty_ssh::SshTunnel>,
}

/// 붙여넣기 시점의 mirror 대상을 고정하고 백그라운드 업로드 뒤 그 surface에 원격 경로를 입력한다.
#[cfg(feature = "gui")]
pub(crate) struct PendingImageUpload {
    /// attach 세션을 찾을 로컬 mirror workspace ID.
    pub(crate) mirror_ws_id: u32,
    /// 붙여넣기 시점에 정한 로컬 mirror surface ID.
    pub(crate) surface_id: u32,
    /// 붙여넣기 시점의 bracketed paste 설정.
    pub(crate) bracketed: bool,
    pub(crate) file_name: String,
    pub(crate) png_bytes: Vec<u8>,
}

/// MeshContext의 내용. 로컬 surface ID는 이 요청을 담는 큐의 키다.
#[derive(Debug, Clone)]
#[cfg(feature = "gui")]
pub(crate) struct AttachMeshContextForward {
    pub(crate) width_px: u32,
    pub(crate) height_px: u32,
    pub(crate) pixels_per_point: f32,
    pub(crate) theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
    pub(crate) focused: bool,
}

/// engine별 도메인 상태. GUI에서는 창마다 따로 보유하고 공유 자원은 Arc로 주입한다.
/// 외부 함수의 타입에 쓰이지만 내부 필드는 crate 밖에 노출하지 않는다.
pub struct CoreState {
    pub(crate) workspaces: Vec<Workspace>,
    /// 표시 순서의 카테고리. 생성·복원 뒤 기본 normal 항목을 앞에 두도록 정규화한다.
    pub(crate) categories: Vec<crate::model::WorkspaceCategory>,
    pub(crate) next_ids: IdGenerator,
    pub(crate) default_cols: usize,
    pub(crate) default_rows: usize,
    pub(crate) waker: Waker,

    pub(crate) settings: Settings,

    pub(crate) notifications: NotificationStore,
    pub(crate) hook_manager: HookManager,
    pub(crate) global_hook_manager: GlobalHookManager,

    pub(crate) closed_items: crate::model::ClosedItemStore,

    pub(crate) command_index: crate::core::command_index::CommandIndex,

    pub(crate) observer_router: crate::output_observer::ObserverRouter,

    pub(crate) approval_store: std::sync::Arc<tasty_approval::ApprovalStore>,

    /// 같은 밀리초에 발생한 telemetry 키를 구별할 이 engine의 순번.
    pub(crate) telemetry_seq: std::sync::Arc<tasty_telemetry::TelemetrySeq>,

    /// 이 engine의 메모리 내 이상 탐지 상태. 탐지 기록 저장은 호출자가 맡는다.
    pub(crate) anomaly_detector: std::sync::Arc<tasty_telemetry::AnomalyDetector>,

    pub(crate) agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,

    /// task 종결을 대기자에게 알리고 같은 이벤트 큐에도 기록한다.
    pub(crate) task_waker_hub: std::sync::Arc<crate::core::agent::task_waker::TaskWakerHub>,

    /// runner 스레드에서 생성한 이벤트를 메인 루프로 넘기는 큐.
    pub(crate) agent_event_queue: std::sync::Arc<crate::core::agent::event_feed::AgentEventQueue>,

    pub(crate) surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub(crate) last_key_input: HashMap<u32, std::time::Instant>,

    pub(crate) busy_surfaces: std::collections::HashSet<u32>,

    /// 원격에서 받은 busy 상태. 로컬 폴링이 집합을 교체하므로 별도로 보관한다.
    pub(crate) mirror_busy_surfaces: std::collections::HashSet<u32>,

    /// busy 전송 후보를 마지막으로 만든 (holder, 값). 송신 성공 기록은 아니다.
    /// holder도 비교해야 같은 값으로 점유자가 바뀌어도 새 client에 초기 상태를 보낸다.
    pub(crate) last_forwarded_busy:
        std::collections::HashMap<u32, (crate::core::attach::AttachClientId, bool)>,

    /// attention 전송 후보의 (holder, kind). None은 해제이며 송신 성공과는 별개다.
    pub(crate) last_forwarded_attention: std::collections::HashMap<
        u32,
        (
            crate::core::attach::AttachClientId,
            Option<attention::AttentionKind>,
        ),
    >,

    /// 원격 surface의 cwd. 두 호스트의 파일시스템이 달라 로컬 Path로 해석하지 않는다.
    pub(crate) mirror_surface_cwd: std::collections::HashMap<u32, RemoteCwd>,

    /// cwd 전송 후보의 (holder, 값). 같은 값이어도 holder가 바뀌면 새 후보를 만든다.
    pub(crate) last_forwarded_cwd:
        std::collections::HashMap<u32, (crate::core::attach::AttachClientId, Option<String>)>,

    // attention은 여러 알림 원인이 공유하며 알림 패널 항목과는 별도 상태다.
    pub(crate) attention: attention::AttentionStore,

    // busy 폴링에서 얻은 전경 이름으로 마우스 캡처 제한도 계산한다.
    pub(crate) mouse_capture_disabled_surfaces: std::collections::HashSet<u32>,

    // 캡처 자체의 제한과 별도로 안내 배너만 숨길 surface를 보관한다.
    pub(crate) mouse_capture_banner_suppressed_surfaces: std::collections::HashSet<u32>,

    /// 첫 출력 뒤 OSC 133 경계가 없는 surface에 셸 통합 안내를 고려할 기준 시각.
    pub(crate) shell_integration_first_output_at:
        std::collections::HashMap<u32, std::time::Instant>,
    /// OSC 133 경계를 한 번이라도 받아 안내 대상에서 제외한 surface.
    pub(crate) shell_integration_boundary_seen: std::collections::HashSet<u32>,
    /// 같은 안내를 다시 표시하지 않기 위한 집합.
    pub(crate) shell_integration_hint_shown: std::collections::HashSet<u32>,

    // 매 프레임 OS 프로세스를 조회하지 않도록 busy 폴링의 전경 이름을 재사용한다.
    pub(crate) foreground_names: std::collections::HashMap<u32, String>,

    /// 상태바에 표시할 선택 surface의 Git branch 캐시. 무효화는 branch 모듈이 맡는다.
    #[cfg(feature = "gui")]
    pub(crate) branch_cache: branch::BranchCache,

    /// 폴링에서 전경 이름이 바뀔 때 올리는 번호. PID나 실제 프로세스 동일성을 판별하는 값은 아니다.
    pub(crate) foreground_generation: std::collections::HashMap<u32, u64>,

    /// 잘라내기 후 이동할 surface ID. 단일 슬롯이며 저장하지 않는다.
    pub(crate) pending_move_surface: Option<crate::model::SurfaceId>,

    /// Explorer의 파일 복사·잘라내기 목록. OS 텍스트 클립보드와 별개이며 저장하지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) explorer_clipboard: Option<ExplorerClipboard>,

    /// 공용 설정 파일에서 읽은 Explorer 즐겨찾기. 변경 뒤 저장은 호출자가 요청한다.
    #[cfg(feature = "gui")]
    pub(crate) explorer_favorites: crate::core::explorer_favorites::ExplorerFavorites,

    /// 공용 설정 파일에서 읽은 주소·포트 즐겨찾기. 변경 뒤 저장은 호출자가 요청한다.
    #[cfg(feature = "gui")]
    pub(crate) port_favorites: crate::core::port_favorites::PortFavorites,

    /// 실제 Terminal과 scrollback 저장 ID. 레이아웃 트리의 TerminalSurface는 ID만 참조한다.
    pub(crate) terminals: crate::core::terminal_store::TerminalStore,

    /// attach 점유는 연결 수명 동안만 유지하며 저장·복원하지 않는다.
    pub(crate) attach: crate::core::attach::OccupancyRegistry,

    /// 서버의 mesh 구독 상태. 실제 전송은 PluginManager를 가진 GUI·헤드리스 계층이 맡는다.
    pub(crate) mesh_mirror: crate::core::mesh_mirror::MeshMirrorRegistry,

    /// client가 조립한 mesh frame을 로컬 surface ID로 보관한다. 서버 구독 상태와는 별개다.
    #[cfg(feature = "gui")]
    pub(crate) attach_mesh_frames: crate::core::attach_mesh_frames::AttachMeshFrameStore,

    /// 자식 terminal surface의 부모·번호·상태 기록. 파일에서 읽으며 저장은 호출자가 요청한다.
    pub(crate) child_terminals: crate::core::child_terminal::ChildTerminalRegistry,

    /// surface가 없는 PTY의 등록 정보와 watcher 결과. Terminal은 별도 store에 있다. 비영속이다.
    pub(crate) pty_registry: crate::core::pty_registry::PtyRegistry,

    /// 서버의 attach 점유 터미널을 표시할 사본. GUI 타이머가 갱신하며 원본 PTY는 계속 유지한다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui render_pass and attach poll read the readonly mirror"
        )
    )]
    pub(crate) readonly_views: HashMap<u32, tasty_terminal::Terminal>,

    /// IPC가 요청한 GUI attach 대기열. 처리 후 사용자의 선택을 옮기지 않는다.
    pub(crate) pending_gui_attach: Vec<(u16, u32)>,

    /// 캡처 시점에 정한 mirror workspace. None이면 로컬 클립보드에 기록한다.
    /// 캡처 도중 포커스가 바뀌어도 업로드 대상은 바뀌지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) pending_screenshot_captures: Vec<Option<u32>>,

    /// mirror 이미지 붙여넣기 요청. App이 업로드하고 저장 경로를 미리 정한 surface로 보낸다.
    #[cfg(feature = "gui")]
    pub(crate) pending_image_uploads: Vec<PendingImageUpload>,

    /// GUI·헤드리스 attach 서버가 공유하는 캡처 업로드 버퍼.
    pub(crate) capture_uploads: crate::core::capture_upload::CaptureUploadRegistry,

    /// 전용 bulk 연결의 (client_id, transfer_id)별 메타데이터·바이트 버퍼.
    pub(crate) bulk_transfers: crate::core::bulk_transfer::BulkTransferRegistry,

    /// 원격 실행 요청과 사용자 선택 보정 태그. anchor는 로컬 ID이며 전송 직전에 원격 ID로 바꾼다.
    pub(crate) pending_structural_forward: Vec<crate::core::PendingStructuralForward>,

    /// 로컬 mirror ID별 최신 resize 목표. 로컬에 먼저 적용하지 않고 서버 echo를 기다린다.
    #[cfg(feature = "gui")]
    pub(crate) pending_resize_forward: std::collections::HashMap<u32, (usize, usize)>,

    /// mirror에서 실제 attention을 지웠을 때의 로컬 ID를 모아 서버에 해제를 요청한다.
    pub(crate) pending_attention_clear_forward: std::collections::HashSet<u32>,

    /// 원격 디렉터리 조회 요청만 담는다. 응답은 MirrorEvent로 따로 들어온다.
    #[cfg(feature = "gui")]
    pub(crate) pending_list_dir_forward: Vec<crate::core::PendingListDirForward>,
    /// 원격 Git 조회 요청. 응답은 MirrorEvent로 따로 들어온다.
    #[cfg(feature = "gui")]
    pub(crate) pending_git_query_forward: Vec<crate::core::PendingGitQueryForward>,
    /// 원격 markdown 원문 조회 요청. 응답은 MirrorEvent로 따로 들어온다.
    #[cfg(feature = "gui")]
    pub(crate) pending_markdown_content_forward: Vec<crate::core::PendingMarkdownContentForward>,
    /// texture 복구가 필요한 로컬 surface ID. 전송 때 원격 ID로 바꾼다.
    #[cfg(feature = "gui")]
    pub(crate) pending_mesh_full_resend_forward: std::collections::HashSet<u32>,

    /// 로컬 mesh surface별 최신 geometry·theme·focus. 같은 surface의 변경은 합친다.
    #[cfg(feature = "gui")]
    pub(crate) pending_mesh_context_forward:
        std::collections::HashMap<u32, AttachMeshContextForward>,

    /// 로컬 mesh surface별 누적 입력. App이 원격으로 보낸다.
    #[cfg(feature = "gui")]
    pub(crate) pending_mesh_input_forward:
        std::collections::HashMap<u32, tasty_plugin_protocol::protocol::RawInputWire>,

    /// 사용자가 직접 확정한 attach는 성공 뒤 새 mirror를 선택할 수 있어 IPC 요청과 분리한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_gui_attach_user: Vec<GuiAttachUserReq>,

    /// 대상별 출력 알림을 만드는 인터페이스. 도메인은 winit EventLoopProxy를 직접 보유하지 않는다.
    pub(crate) waker_factory: Option<crate::waker::SharedWakerFactory>,

    /// surface 종류와 생성·복원 동작의 등록부.
    pub(crate) surface_registry: Arc<SurfaceKindRegistry>,

    /// 내장 키 외에 plugin이 선언한 hook 키를 검증할 때 사용한다.
    pub(crate) plugin_hook_events: Arc<crate::core::hook_event_registry::PluginHookEventRegistry>,

    /// 기본·plugin·사용자 파일 형식 등록부. PluginManager와 같은 Arc를 쓴다.
    pub(crate) file_format: Arc<crate::file::format::FileFormatRegistry>,
    /// PluginManager와 공유하는 파일 처리기 등록부.
    pub(crate) file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    /// 사용자가 선택한 처리기 이력. 변경 뒤 저장을 시도하며 실패는 로그로 남긴다.
    #[cfg(feature = "gui")]
    pub(crate) file_handler_recent: crate::file::handler::recent::RecentPicks,
    /// App이 GUI 이벤트 루프를 준비한 뒤 주입하는 파일 식별 worker 인터페이스.
    #[cfg(feature = "gui")]
    pub(crate) identify_worker:
        Option<std::sync::Arc<dyn crate::core::identify_port::IdentifySpawner>>,

    pub(crate) layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker,
    /// 복원한 활성 workspace 인덱스. 창 상태를 만들 때 한 번 소비한다.
    pub(crate) restored_active_workspace: Option<usize>,
    /// deferred Terminal 생성 뒤 적용할 scrollback. 읽지 못했거나 비어 있으면 등록하지 않는다.
    pub(crate) pending_scrollback_inject: HashMap<u32, Vec<tasty_terminal::ScrollbackLine>>,
    /// plugin 준비 대기 후 적용할 레이아웃. 대기와 제한 시간 처리는 App이 맡는다.
    pub(crate) pending_layout_restore: Option<crate::core::layout_persistence::SavedLayout>,
    /// 이 engine의 레이아웃 슬롯. 프로세스 내 engine들의 이 필드로 점유를 확인한다.
    /// 디스크 잠금은 아니며 헤드리스는 None이다.
    pub(crate) layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
    /// 읽기 실패·높은 version으로 기존 슬롯을 덮어쓰면 안 되는 상태.
    pub(crate) layout_slot_protected: bool,
    /// 해석 실패한 원본을 저장 전에 재확인·백업해야 하는 상태.
    pub(crate) layout_slot_unparsable: bool,
    /// 검사에서 실제 홈 대신 사용할 저장 디렉터리. 저장과 백업 공간 판정이 함께 사용한다.
    #[cfg(test)]
    pub(crate) layouts_dir_override: Option<std::path::PathBuf>,
    /// 백업 공간 부족 또는 보존 실패를 사용자에게 알리기 위한 상태.
    pub(crate) layout_slot_preserve_failed: bool,

    #[cfg(debug_assertions)]
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "only the gui input simulation IPC of debug builds reads the flag"
        )
    )]
    pub(crate) input_simulation_enabled: bool,

    /// Core와 공유하는 저장소. engine 내부에서 직접 메타데이터를 기록할 때 쓴다.
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,

    /// Core와 같은 runner 등록부를 렌더 경로에서도 조회하도록 부팅 때 주입한다.
    pub(crate) agent_runner_registry:
        std::sync::OnceLock<std::sync::Arc<crate::core::agent::runner_thread::RunnerRegistry>>,

    /// 검사 중 홈 경로 override를 유지한다. 다른 필드의 Drop까지 격리하려면 마지막 필드여야 한다.
    /// 생성 때만 잠시 바꾸면 이후 저장·정리 코드가 실제 홈을 사용할 수 있다.
    #[cfg(test)]
    _isolated_home: Option<crate::test_support::IsolatedHome>,
}

impl CoreState {
    /// 기본 Settings와 in-memory 저장소로 생성한다. 사용자 config.toml의 설정을 읽지 않는다.
    // 이유: 현재 호출처가 모두 #[cfg(test)]에 있다.
    #[allow(dead_code)]
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory()?,
            ));
        Self::new_with_ids_and_settings(cols, rows, waker, None, None, memory, Settings::default())
    }

    /// 다른 engine과 발급기를 공유할 수 있다. 슬롯이 있고 restore_layout이 켜져 있을 때만 읽는다.
    pub fn new_with_ids(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> anyhow::Result<Self> {
        let state = Self::new_with_ids_and_settings(
            cols,
            rows,
            waker,
            shared_ids,
            layout_slot,
            memory,
            Settings::load(),
        )?;
        Ok(state)
    }

    /// 슬롯 로드 판정을 대기 복원·쓰기 보호·백업 필요 플래그에 반영한다.
    pub(crate) fn accept_slot_load(
        &mut self,
        load: crate::core::layout_persistence::SlotLoad,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) {
        use crate::core::layout_persistence::SlotLoad;
        match load {
            SlotLoad::Loaded(saved) => self.pending_layout_restore = Some(saved),
            SlotLoad::Absent => {}
            SlotLoad::Unreadable => self.layout_slot_protected = true,
            SlotLoad::Unparsable => {
                self.layout_slot_unparsable = true;
                // 첫 저장 전에 뜨는 안내도 백업 공간 부족을 구별해야 한다.
                self.layout_slot_preserve_failed = self.slot_preservation_is_blocked(slot);
            }
        }
    }

    /// 저장과 같은 디렉터리에서 백업 공간을 확인한다. 검사 override도 동일하게 적용한다.
    fn slot_preservation_is_blocked(
        &self,
        slot: crate::core::layout_persistence::LayoutSlotId,
    ) -> bool {
        #[cfg(test)]
        if let Some(dir) = self.layouts_dir_override.as_deref() {
            return crate::core::layout_persistence::slot_preservation_is_blocked_in(dir, slot);
        }
        crate::core::layout_persistence::slot_preservation_is_blocked(slot)
    }

    fn new_with_ids_and_settings(
        cols: usize,
        rows: usize,
        waker: Waker,
        shared_ids: Option<IdGenerator>,
        layout_slot: Option<crate::core::layout_persistence::LayoutSlotId>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        settings: Settings,
    ) -> anyhow::Result<Self> {
        // 이 생성자 내부에서 파일을 읽기 전에 검사 전용 홈을 설정한다.
        #[cfg(test)]
        let isolated_home = Some(crate::test_support::IsolatedHome::new());
        let restore_layout = settings.general.restore_layout;

        let next_ids = shared_ids.unwrap_or_default();
        // task 통지를 기록하는 쪽과 메인 루프가 같은 큐를 사용해야 한다.
        let agent_event_queue =
            std::sync::Arc::new(crate::core::agent::event_feed::AgentEventQueue::new());
        let mut engine = Self {
            workspaces: Vec::new(),
            categories: vec![crate::model::WorkspaceCategory::normal()],
            next_ids: next_ids.clone(),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_counter(500, next_ids.notification_counter()),
            hook_manager: HookManager::with_counter(next_ids.hook_counter()),
            global_hook_manager: GlobalHookManager::with_counter(next_ids.global_hook_counter()),
            closed_items: crate::model::ClosedItemStore::new(),
            command_index: crate::core::command_index::CommandIndex::new(),
            observer_router: crate::output_observer::ObserverRouter::with_counter(
                next_ids.observer_counter(),
            ),
            approval_store: std::sync::Arc::new(tasty_approval::ApprovalStore::new()),
            telemetry_seq: std::sync::Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: std::sync::Arc::new(tasty_telemetry::AnomalyDetector::new()),
            agent_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            task_waker_hub: std::sync::Arc::new(
                crate::core::agent::task_waker::TaskWakerHub::with_feed(std::sync::Arc::clone(
                    &agent_event_queue,
                )),
            ),
            agent_event_queue,
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            busy_surfaces: std::collections::HashSet::new(),
            mirror_busy_surfaces: std::collections::HashSet::new(),
            last_forwarded_busy: std::collections::HashMap::new(),
            mirror_surface_cwd: std::collections::HashMap::new(),
            last_forwarded_cwd: std::collections::HashMap::new(),
            last_forwarded_attention: std::collections::HashMap::new(),
            attention: attention::AttentionStore::default(),
            mouse_capture_disabled_surfaces: std::collections::HashSet::new(),
            mouse_capture_banner_suppressed_surfaces: std::collections::HashSet::new(),
            shell_integration_first_output_at: std::collections::HashMap::new(),
            shell_integration_boundary_seen: std::collections::HashSet::new(),
            shell_integration_hint_shown: std::collections::HashSet::new(),
            foreground_names: std::collections::HashMap::new(),
            foreground_generation: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            branch_cache: branch::BranchCache::default(),
            pending_move_surface: None,
            #[cfg(feature = "gui")]
            explorer_clipboard: None,
            #[cfg(feature = "gui")]
            explorer_favorites: crate::core::explorer_favorites::ExplorerFavorites::load(),
            #[cfg(feature = "gui")]
            port_favorites: crate::core::port_favorites::PortFavorites::load(),
            terminals: crate::core::terminal_store::TerminalStore::new(),
            attach: crate::core::attach::OccupancyRegistry::new(),
            mesh_mirror: crate::core::mesh_mirror::MeshMirrorRegistry::default(),
            #[cfg(feature = "gui")]
            attach_mesh_frames: crate::core::attach_mesh_frames::AttachMeshFrameStore::default(),
            child_terminals: crate::core::child_terminal::ChildTerminalRegistry::load(),
            pty_registry: crate::core::pty_registry::PtyRegistry::with_counter(
                next_ids.pty_counter(),
            ),
            readonly_views: HashMap::new(),
            pending_gui_attach: Vec::new(),
            #[cfg(feature = "gui")]
            pending_screenshot_captures: Vec::new(),
            #[cfg(feature = "gui")]
            pending_image_uploads: Vec::new(),
            capture_uploads: crate::core::capture_upload::CaptureUploadRegistry::new(),
            bulk_transfers: crate::core::bulk_transfer::BulkTransferRegistry::new(),
            pending_structural_forward: Vec::new(),
            #[cfg(feature = "gui")]
            pending_resize_forward: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            pending_list_dir_forward: Vec::new(),
            #[cfg(feature = "gui")]
            pending_git_query_forward: Vec::new(),
            #[cfg(feature = "gui")]
            pending_markdown_content_forward: Vec::new(),
            #[cfg(feature = "gui")]
            pending_mesh_full_resend_forward: std::collections::HashSet::new(),
            pending_attention_clear_forward: std::collections::HashSet::new(),
            #[cfg(feature = "gui")]
            pending_mesh_context_forward: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            pending_mesh_input_forward: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            pending_gui_attach_user: Vec::new(),
            waker_factory: None,
            surface_registry: {
                let reg = SurfaceKindRegistry::new();
                crate::core::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            plugin_hook_events: Arc::new(
                crate::core::hook_event_registry::PluginHookEventRegistry::new(),
            ),
            file_format: {
                let reg = crate::file::format::FileFormatRegistry::new();
                reg.install_host_defaults(crate::file::format::HOST_DEFAULTS_TOML);
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            file_handler: {
                let reg = crate::file::handler::FileHandlerRegistry::new();
                reg.install_host_defaults(crate::file::handler::HOST_DEFAULTS_TOML);
                if let Some(path) = file_handler_user_config_path() {
                    reg.install_user_config(&path);
                }
                Arc::new(reg)
            },
            #[cfg(feature = "gui")]
            file_handler_recent: crate::file::handler::recent::RecentPicks::load(
                &file_handler_recent_path(),
            ),
            #[cfg(feature = "gui")]
            identify_worker: None,
            layout_dirty: crate::core::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_scrollback_inject: HashMap::new(),
            pending_layout_restore: None,
            layout_slot,
            layout_slot_protected: false,
            layout_slot_unparsable: false,
            #[cfg(test)]
            layouts_dir_override: None,
            layout_slot_preserve_failed: false,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            memory,
            agent_runner_registry: std::sync::OnceLock::new(),
            #[cfg(test)]
            _isolated_home: isolated_home,
        };

        engine
            .file_handler
            .attach_detector_info(engine.file_format.clone());

        engine.notifications = NotificationStore::with_counter(
            engine.settings.notification.coalesce_ms,
            next_ids.notification_counter(),
        );

        // 대기를 마친 뒤 복원하도록 데이터만 읽는다. 이 슬롯만으로 GC하면 다른 창의 scrollback을 지울 수 있다.
        if restore_layout && let Some(slot) = layout_slot {
            engine.accept_slot_load(crate::core::layout_persistence::load_slot(slot), slot);
        }

        // 복원할 레이아웃이 있으면 기본 PTY를 먼저 만들지 않는다. 복원이 트리를 교체해도 별도 store의 PTY는 남기 때문이다.
        if engine.pending_layout_restore.is_none() {
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
                    extra_env: &sh.envs_ref(),
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

    /// 설정과 factory가 있으면 surface별 waker, 아니면 공용 waker를 반환한다.
    pub fn make_waker(&self, surface_id: u32) -> Waker {
        if self.settings.performance.targeted_pty_polling
            && let Some(factory) = &self.waker_factory
        {
            return factory.make_targeted_waker(surface_id);
        }
        self.waker.clone()
    }

    /// 트리에서 제거하기 전에 탭의 복원 snapshot을 만든다. 복원 목록에 넣는 일은 호출자가 맡는다.
    pub(crate) fn capture_closed_tab(
        &self,
        pane_id: u32,
        tab_index: usize,
    ) -> Option<crate::model::ClosedItem> {
        let tab = self.find_pane_by_id(pane_id)?.tabs.get(tab_index)?;
        let mut snap_fn = crate::core::surface_registry::snapshot_fn_for(&self.surface_registry);
        let terminals = &self.terminals;
        crate::model::closed_item::ClosedTab::from_tab(tab, &mut snap_fn, &|id| terminals.get(id))
            .map(crate::model::ClosedItem::Tab)
    }

    /// pane 제거 전에 분할 위치를 포함한 snapshot을 만든다. workspace의 유일한 pane이면 None이다.
    pub(crate) fn capture_closed_pane(&self, pane_id: u32) -> Option<crate::model::ClosedItem> {
        let ws = self
            .workspaces
            .get(self.find_workspace_index_for_pane(pane_id)?)?;
        if ws.pane_layout().all_pane_ids().len() <= 1 {
            return None;
        }
        let pane = ws.pane_layout().find_pane(pane_id)?;
        let (direction, ratio, was_first, sibling_pane_id) =
            ws.pane_layout().locate_split_context(pane_id)?;
        let mut snap_fn = crate::core::surface_registry::snapshot_fn_for(&self.surface_registry);
        let terminals = &self.terminals;
        Some(crate::model::ClosedItem::from_pane(
            pane,
            sibling_pane_id,
            direction,
            ratio,
            was_first,
            &mut snap_fn,
            &|id| terminals.get(id),
        ))
    }

    /// 현재 트리에서 복원 항목의 출처 workspace를 찾는다. 트리를 바꾸기 전에 호출해야 한다.
    /// 이미 제거했거나 workspace 전체 항목이면 None이라 workspace 범위 복원에서 제외된다.
    fn origin_workspace_of(&self, item: &crate::model::ClosedItem) -> Option<u32> {
        use crate::model::closed_item::ClosedItem;
        let ws_idx = match item {
            ClosedItem::Surface { surface, .. } => self
                .find_workspace_index_for_surface(surface.id)
                .map(|(i, _)| i),
            ClosedItem::Tab(tab) => self
                .find_pane_for_tab(tab.id)
                .and_then(|pid| self.find_workspace_index_for_pane(pid)),
            ClosedItem::Pane { pane, .. } => self.find_workspace_index_for_pane(pane.id),
            ClosedItem::Workspace { .. } => return None,
        }?;
        self.workspaces.get(ws_idx).map(|ws| ws.id)
    }

    pub fn push_closed_item(
        &mut self,
        mut item: crate::model::ClosedItem,
    ) -> crate::close_trace::PushClosedItemTimings {
        let mut timings = crate::close_trace::PushClosedItemTimings::default();
        let origin_workspace = self.origin_workspace_of(&item);
        let mem = self.memory.clone();
        let t_inject = std::time::Instant::now();
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            let mut guard = crate::poison::recover_mutex(
                mem.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            crate::surface_meta::SurfaceMetaStore::get(&mut *guard, sid, "restore.command")
        });
        timings.restore_inject = t_inject.elapsed();
        // 닫힌 항목은 큰 scrollback을 메모리에 계속 들지 않도록 별도 파일 ID로 저장한다.
        // 원래 surface의 저장 ID와 분리해 surface 정리가 이 파일까지 지우지 않게 한다.
        let t_persist = std::time::Instant::now();
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
        timings.scrollback_persist = t_persist.elapsed();
        // 복원 목록에서 밀려난 항목의 별도 scrollback 파일도 지운다.
        let t_evict = std::time::Instant::now();
        if let Some(evicted) = self.closed_items.push(item, origin_workspace) {
            let mut refs = Vec::new();
            crate::model::closed_item::collect_scrollback_refs(&evicted, &mut refs);
            for id in refs {
                crate::scrollback_store::delete(&id);
            }
        }
        timings.evict = t_evict.elapsed();
        timings
    }

    /// 키보드·IME·붙여넣기의 사용자 입력 시각을 기록한다. 마우스 보고·파일 열기·에이전트 전송은 제외한다.
    #[cfg(feature = "gui")]
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    #[cfg(feature = "gui")]
    pub fn resync_terminal_palettes(&mut self) {
        self.terminals.resync_palettes();
    }

    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    #[cfg(feature = "gui")]
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
    /// 등록된 종류로 surface를 만든다. Terminal의 PTY 생성은 호출자가 별도로 처리한다.
    /// cwd는 호출자가 정해 넘기며 사용 여부는 각 종류가 결정한다.
    pub(crate) fn create_surface_via_registry(
        &self,
        kind: &str,
        surface_id: u32,
        cwd: Option<&std::path::Path>,
        params: &serde_json::Value,
    ) -> anyhow::Result<Box<dyn crate::model::Surface>> {
        // 철회된 plugin 종류는 알 수 없는 종류와 구별해 필요한 조치를 안내한다.
        if let Some(plugin_id) = self.surface_registry.withdrawn_by(kind) {
            return Err(crate::core::surface_registry::SurfaceKindWithdrawn {
                kind: kind.to_string(),
                plugin_id,
            }
            .into());
        }
        let def = self
            .surface_registry
            .get_live(kind)
            .ok_or_else(|| anyhow::anyhow!("unknown surface kind: {}", kind))?;
        // 명시한 params가 우선이다. cwd 상속 경로에서 홈으로 바꾸지 않도록 @home은 여기서 해석하지 않는다.
        if def.default_params.is_empty() {
            return (def.create)(surface_id, cwd, params);
        }
        let mut owned = params.clone();
        if self.apply_kind_default_params(&def, &mut owned, None) {
            (def.create)(surface_id, cwd, &owned)
        } else {
            (def.create)(surface_id, cwd, params)
        }
    }

    /// 없는 키에만 기본값을 넣고 하나라도 넣으면 true다. params가 객체가 아니면 변경하지 않는다.
    /// @settings.explorer_view_mode와 전달된 @home을 해석하고 알 수 없는 @ 토큰은 경고 후 건너뛴다.
    pub(crate) fn apply_kind_default_params(
        &self,
        def: &crate::core::surface_registry::SurfaceKindDef,
        params: &mut serde_json::Value,
        home: Option<&std::path::Path>,
    ) -> bool {
        if def.default_params.is_empty() {
            return false;
        }
        let Some(obj) = params.as_object_mut() else {
            return false;
        };
        let mut injected = false;
        for (key, token) in &def.default_params {
            if obj.contains_key(key.as_str()) {
                continue;
            }
            let Some(val) = self.resolve_default_param_token(token, home) else {
                continue;
            };
            obj.insert(key.clone(), serde_json::Value::String(val));
            injected = true;
        }
        injected
    }

    fn resolve_default_param_token(
        &self,
        token: &str,
        home: Option<&std::path::Path>,
    ) -> Option<String> {
        match token {
            "@settings.explorer_view_mode" => {
                Some(self.settings.general.explorer_view_mode.clone())
            }
            "@home" => home.map(|p| p.to_string_lossy().to_string()),
            t if t.starts_with('@') => {
                tracing::warn!("unknown default_param policy token: {t}");
                None
            }
            literal => Some(literal.to_string()),
        }
    }
}

impl CoreState {
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

    /// surface_id가 속한 탭에서 실제 선택된 surface의 제목을 읽는다.
    /// 제목이 없으면 OSC 제목을 비우고 사용자가 명시한 탭 이름은 유지한다.
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

    #[cfg(feature = "gui")]
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.default_cols = cols;
        self.default_rows = rows;
    }
}

fn file_handler_user_config_path() -> Option<std::path::PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("file-handlers.toml"))
}

/// 사용자 처리기 선택 이력. 홈을 못 찾으면 공용 임시 경로에도 읽기·쓰기를 시도한다.
#[cfg(feature = "gui")]
fn file_handler_recent_path() -> std::path::PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handler-recent.json"))
        // 이유: 사용자 선택 이력의 공유 폴백이며 인스턴스별로 격리하지 않는다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handler-recent.json"))
}

mod attention;
#[cfg(feature = "gui")]
mod branch;
mod busy;
mod category;
pub mod child_liveness;
mod finders;
mod global_hooks;
mod idle_hooks;
mod message;
mod output_read;
mod pty;
mod shell_integration_hint;
mod soft_occupancy;
mod surface_cwd;
mod terminal_finders;

pub(crate) use attention::AttentionKind;
#[cfg(feature = "gui")]
pub(crate) use branch::HeadState;
pub use category::CategoryOpError;
#[cfg(feature = "gui")]
pub use finders::SurfaceDisplayPath;
pub(crate) use surface_cwd::RemoteCwd;
#[cfg(any(feature = "gui", test))]
pub(crate) use surface_cwd::SurfaceCwd;

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
    fn two_engines_do_not_hand_out_the_same_hook_id() {
        use tasty_hooks::{HookBinding, HookEvent, HookManager};
        let ids = IdGenerator::new();
        let mut a = HookManager::with_counter(ids.hook_counter());
        let mut b = HookManager::with_counter(ids.hook_counter());
        let ia = a.add_hook(
            1,
            HookEvent::CommandCompleted(None),
            HookBinding::InlineShell("echo a".into()),
            false,
        );
        let ib = b.add_hook(
            1,
            HookEvent::CommandCompleted(None),
            HookBinding::InlineShell("echo b".into()),
            false,
        );
        assert_ne!(ia, ib, "공유 발급기를 쓰는 두 engine의 hook ID가 겹쳤다");
    }

    #[test]
    fn two_engines_do_not_hand_out_the_same_global_hook_id() {
        use crate::host_api::hooks::global::{GlobalHookManager, HookCondition};
        let ids = IdGenerator::new();
        let mut a = GlobalHookManager::with_counter(ids.global_hook_counter());
        let mut b = GlobalHookManager::with_counter(ids.global_hook_counter());
        let ia = a.add(
            HookCondition::Interval(std::time::Duration::from_secs(60)),
            "echo a".into(),
            None,
        );
        let ib = b.add(
            HookCondition::Interval(std::time::Duration::from_secs(60)),
            "echo b".into(),
            None,
        );
        assert_ne!(
            ia, ib,
            "공유 발급기를 쓰는 두 engine의 global hook ID가 겹쳤다"
        );
    }

    #[test]
    fn bump_surface_floor_is_noop_when_already_higher() {
        let ids = IdGenerator::new();
        for _ in 0..4 {
            ids.next_surface();
        }
        ids.bump_surface_floor(3);
        assert_eq!(ids.next_surface(), 5);
    }
}

#[cfg(test)]
mod default_params_tests {
    use super::CoreState;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn explorer_defaults_without_home_inject_view_mode_only() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let injected = e.apply_kind_default_params(&def, &mut params, None);
        assert!(injected);
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert!(
            params.get("path").is_none(),
            "@home must not resolve when home=None"
        );
    }

    #[test]
    fn explorer_defaults_with_home_inject_path() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let mut params = serde_json::json!({});
        let home = std::path::PathBuf::from("/home/tester");
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], e.settings.general.explorer_view_mode);
        assert_eq!(params["path"], "/home/tester");
    }

    #[test]
    fn explicit_params_preserved() {
        let e = engine();
        let def = e.surface_registry.get("explorer").unwrap();
        let home = std::path::PathBuf::from("/home/tester");
        let mut params = serde_json::json!({"view_mode": "list", "path": "/explicit"});
        e.apply_kind_default_params(&def, &mut params, Some(&home));
        assert_eq!(params["view_mode"], "list");
        assert_eq!(params["path"], "/explicit");
    }

    #[test]
    fn kind_without_defaults_is_noop() {
        let e = engine();
        let def = e.surface_registry.get("terminal").unwrap();
        let mut params = serde_json::json!({});
        assert!(!e.apply_kind_default_params(&def, &mut params, None));
    }
}

/// 잘못된 셸 설정이 engine 생성의 Err로 전달되는지 확인한다. 창 전체의 오류 처리 검사는 아니다.
#[cfg(test)]
mod engine_creation_failure_tests {
    use super::*;

    fn bogus_shell_settings() -> Settings {
        let mut s = Settings::default();
        s.general.shell = "/nonexistent/definitely/not/a/real/shell-xyzzy".to_string();
        // 복원 대신 새 workspace 생성 경로를 실행해 셸 생성 오류를 확인한다.
        s.general.restore_layout = false;
        s
    }

    fn in_memory() -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().expect("in-memory store"),
        ))
    }

    #[test]
    fn a_bogus_shell_path_makes_engine_creation_return_err_not_panic() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let result = CoreState::new_with_ids_and_settings(
            80,
            24,
            waker,
            None,
            None,
            in_memory(),
            bogus_shell_settings(),
        );
        let err = result
            .err()
            .expect("a bogus shell must fail engine creation");
        let msg = format!("{err}");
        assert!(
            msg.contains("shell-xyzzy"),
            "the error must name the shell that could not be spawned, got: {msg}"
        );
    }

    #[test]
    fn a_valid_shell_still_produces_an_engine_with_one_workspace() {
        let waker: Waker = std::sync::Arc::new(|| {});
        let mut ok = Settings::default();
        ok.general.restore_layout = false;
        let engine =
            CoreState::new_with_ids_and_settings(80, 24, waker, None, None, in_memory(), ok)
                .expect("default settings must produce an engine");
        assert_eq!(engine.workspaces.len(), 1);
    }
}
