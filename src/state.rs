mod accessors;
mod cascade_window;
#[cfg(feature = "gui")]
mod detect;
#[cfg(feature = "gui")]
mod dialogs;
#[cfg(feature = "gui")]
mod events;
#[cfg(feature = "gui")]
mod focus;
#[cfg(all(test, feature = "gui"))]
mod fullscreen_stage_tests;
mod ipc_window;
#[cfg(any(feature = "gui", test))]
mod layout;
pub mod mouse;
pub(crate) mod pane;
#[cfg(all(test, feature = "gui"))]
mod popup_close_tests;
#[cfg(all(test, feature = "gui"))]
mod popup_ownership_tests;
mod tab;
#[cfg(test)]
pub(crate) mod tests;
mod workspace;

// 팔레트 매칭은 헤드리스 시험에서도 검사한다.
#[cfg(any(feature = "gui", test))]
pub mod command_palette;
pub mod preset_apply;
#[cfg(feature = "gui")]
pub mod search;
#[cfg(feature = "gui")]
pub use tasty_selection as selection;

pub use crate::core::host_event::{PendingHostEvent, PendingSurfaceClosed};
#[cfg(feature = "gui")]
pub use dialogs::{
    DialogState, FileHandlerPickerData, FileHandlerPickerResult, PendingNativeMenu,
    PendingScriptConfirm, PickerHandlerSummary, RenameTarget, TabDragState, TerminalLinkMenu,
    WsDragState,
};
#[cfg(feature = "gui")]
pub(crate) use dialogs::{FilePickerData, FilePickerRequester, FilePickerResult, FpLoadState};
#[cfg(feature = "gui")]
pub use events::FocusedSurfaceType;
pub use workspace::WorkspaceCloseOrigin;

use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::model::LogicalPx;
#[cfg(any(feature = "gui", test))]
use crate::model::PhysicalPx;

/// 열린 모달의 종류. 창을 열 때 기록해 debug 조회에서 모달을 구분한다.
// 이유: 헤드리스에는 모달을 여는 호출부가 없지만 debug ui.state가 같은 타입을 사용한다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "only the gui opens a modal, so headless never builds a variant"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalKind {
    Settings,
    Plugins,
    Quit,
}

impl ModalKind {
    /// debug IPC 응답에 쓰는 이름. 내부 판정은 enum을 사용한다.
    #[cfg(debug_assertions)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Plugins => "plugins",
            Self::Quit => "quit",
        }
    }
}

pub struct AppState {
    pub(crate) active_workspace: usize,
    /// 카테고리별로 마지막에 선택한 워크스페이스 ID. 영속화하지 않는다.
    /// 재정렬에 영향을 받지 않도록 인덱스 대신 ID를 저장하며, 찾지 못하면 첫 항목을 선택한다.
    #[cfg(any(feature = "gui", debug_assertions, test))]
    pub(crate) category_last_active: std::collections::HashMap<
        tasty_utils::id::WorkspaceCategoryId,
        tasty_utils::id::WorkspaceId,
    >,
    /// 설정 모달 열기 요청. 화면에 이미 표시됐는지는 active_modal_id/kind로 확인한다.
    /// redraw의 dispatch_pending_modal_opens 또는 Escape 처리가 요청을 지운다.
    // release 헤드리스에는 이 요청을 읽는 경로가 없다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) settings_open_requested: bool,
    /// View에 등록된 활성 모달 ID의 사본. 열기 요청과 달리 모달이 닫힐 때까지 유지한다.
    /// AppState만 받는 debug 조회를 위해 보관하며 open/close에서 함께 갱신한다.
    // release 헤드리스에는 이 값을 읽는 경로가 없다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) active_modal_id: Option<u64>,
    /// 활성 모달의 종류. active_modal_id만으로는 설정·플러그인·종료 모달을 구별할 수 없다.
    // release 헤드리스에는 이 값을 읽는 경로가 없다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) active_modal_kind: Option<ModalKind>,
    /// 플러그인 모달 열기 요청. dispatch_pending_modal_opens가 처리한 뒤 지운다.
    #[cfg(feature = "gui")]
    pub(crate) plugins_open: bool,
    /// Cached sidebar width from settings (logical pixels).
    #[cfg(feature = "gui")]
    pub(crate) sidebar_width: LogicalPx,
    /// Sidebar visibility: false = completely hidden.
    #[cfg(feature = "gui")]
    pub(crate) sidebar_visible: bool,
    /// Sidebar collapsed: true = compact mode (narrow width, icons only).
    #[cfg(feature = "gui")]
    pub(crate) sidebar_collapsed: bool,
    /// 창 가장자리의 리사이즈 방향. egui가 커서 모양을 덮어쓰므로 프레임 안에서 적용한다.
    /// Windows/Linux의 장식 없는 창에서 사용하며 콘텐츠·오버레이 위에서는 해제한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_resize_cursor: Option<winit::window::ResizeDirection>,
    /// 누른 수식키에 맞는 탭·워크스페이스 전환 대상. 창 포커스를 잃으면 해제한다.
    #[cfg(feature = "gui")]
    pub(crate) switch_overlay: Option<crate::adapters::ui::switch_overlay::SwitchOverlayState>,
    /// 단축키 안내의 누름·숨김·드래그 상태. 영속 위치·크기는 Settings::modifier_hint에 있다.
    #[cfg(feature = "gui")]
    pub(crate) modifier_hint: crate::adapters::ui::modifier_hint_overlay::ModifierHintRuntime,
    /// 튜토리얼의 진행 상태와 시작 요청. 사용자 입력과 렌더 경로에서 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) tutorial: crate::adapters::ui::tutorial::TutorialRuntime,
    /// All transient dialog/popup state.
    #[cfg(feature = "gui")]
    pub(crate) dialogs: DialogState,
    /// 측정한 탭 바 높이(물리 픽셀). 측정 전에는 0이며 논리 길이 토큰을 초기값으로 넣지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) tab_bar_height: PhysicalPx,
    /// Popup manager for internal popups (notification panel, etc.).
    #[cfg(feature = "gui")]
    pub(crate) popups: crate::adapters::ui::PopupManager,
    /// 창마다 하나씩 표시하는 전체화면 무대. 영속화하지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) fullscreen_stage: Option<crate::adapters::ui::fullscreen::StageState>,
    /// 닫힌 무대의 on_close 처리 대기열. close_fullscreen_stage가 추가하고 렌더 경로가 꺼낸다.
    #[cfg(feature = "gui")]
    pub(crate) stage_closed_queue: Vec<crate::adapters::ui::fullscreen::StageId>,
    /// 무대 표시 중 미룬 기본 grid 갱신. 종료 후 첫 프레임에서 배율을 다시 맞출 때 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) stage_deferred_grid_resync: bool,
    /// Terminal text search state.
    #[cfg(feature = "gui")]
    pub(crate) search: crate::search_state::SearchState,
    /// Listening-port scanner async state machine. Driven by the port scanner
    /// popup: Idle → Loading (background thread + mpsc channel) → Ready / Failed.
    /// Reset to `Idle` when the popup closes.
    #[cfg(feature = "gui")]
    pub(crate) port_scan: crate::adapters::ui::popup::port_scanner::PortScanState,
    /// System-wide scan backing the favorites section's LISTEN/NONE badges.
    /// Independent of `port_scan`'s scope (Tasty/System) — always a full
    /// `scan_all()`, kicked only while at least one favorite is registered.
    /// Reset to `Idle` when the popup closes.
    #[cfg(feature = "gui")]
    pub(crate) port_favorites_scan: crate::adapters::ui::popup::port_scanner::PortScanState,
    /// Command palette UI state — query buffer, selection cursor, and a pending
    /// dispatch slot that MainView drains each frame.
    #[cfg(feature = "gui")]
    pub(crate) command_palette: crate::state::command_palette::CommandPaletteState,
    /// 토스트 표시 상태. 사용자·원격 연결 알림의 표시 규칙은 docs/design/systems/toast.md를 따른다.
    #[cfg(feature = "gui")]
    pub(crate) toasts: crate::adapters::ui::ToastManager,
    /// 공지와 조치 버튼을 표시하는 배너 관리자.
    #[cfg(feature = "gui")]
    pub(crate) banners: crate::adapters::ui::BannerManager,
    /// kind별 최근 파일 목록. 읽기·저장은 RecentFilesStore가 담당한다.
    pub(crate) recent_files: crate::recent_files::RecentFiles,
    /// 팝업 위 포인터를 아래 터미널·분할선에 전달하지 않도록 하는 프레임 상태.
    #[cfg(feature = "gui")]
    pub(crate) popup_hovered: bool,
    /// 현재 프레임에 보이는 플러그인 mesh 팝업이 있는지 나타낸다.
    /// 키·IME 처리에서 매니저 대신 이 캐시를 읽으며 draw_plugin_popups가 매번 초기화·갱신한다.
    // release 헤드리스에는 이 값을 읽는 경로가 없다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) plugin_popup_open: bool,
    /// 배너 영역의 마우스 입력을 아래로 전달하지 않도록 한다. 키보드 포커스와는 별개다.
    #[cfg(feature = "gui")]
    pub(crate) banner_hovered: bool,
    /// 단축키 안내 위의 마우스 입력을 아래 surface로 전달하지 않도록 한다.
    #[cfg(feature = "gui")]
    pub(crate) modifier_hint_hovered: bool,
    /// 현재 프레임의 호스트 팝업 레이어. 다른 오버레이와 그리기 순서를 정할 때 쓴다.
    #[cfg(feature = "gui")]
    pub(crate) popup_layers: Vec<egui::LayerId>,
    /// 현재 프레임의 플러그인 팝업 셸 레이어. raw layer는 Area 순서에 등록되지 않아
    /// egui_bridge가 host/plugin 간 순서를 set_sublayer로 명시한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_layers: Vec<egui::LayerId>,
    /// 현재 프레임 호스트 팝업의 rect와 z_seq. 뒤에 그리는 플러그인 팝업이 입력 가림을 판정한다.
    #[cfg(feature = "gui")]
    pub(crate) host_popup_hittest: Vec<crate::adapters::ui::popup::occlusion::Occluder>,
    /// host/plugin 전체에서 최상단이 호스트 팝업일 때 그 ID. 해당 팝업만 Escape를 처리한다.
    #[cfg(feature = "gui")]
    pub(crate) popup_escape_owner: Option<crate::adapters::ui::popup::PopupId>,
    /// 이전 프레임 플러그인 팝업의 셸 rect와 z_seq. 호스트를 먼저 그리므로 입력 판정에
    /// 한 프레임 전 값을 사용한다. 콘텐츠의 물리 rect인 plugin_mesh_popup_regions와 다르다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_hittest: Vec<crate::adapters::ui::popup::occlusion::Occluder>,
    /// 현재 프레임 배너 Area의 레이어. 오버레이 그리기 순서를 정할 때 쓴다.
    #[cfg(feature = "gui")]
    pub(crate) banner_layer: Option<egui::LayerId>,
    /// 단축키 안내를 그렸을 때의 레이어. 표시하지 않으면 None이다.
    #[cfg(feature = "gui")]
    pub(crate) modifier_hint_layer: Option<egui::LayerId>,
    /// 실제 클릭 가능한 창 버튼·상태 바 요소 위인지 나타낸다.
    /// 넓은 패널의 빈 여백과 구분해 가장자리 리사이즈가 우선권을 양보할지 판단한다.
    #[cfg(feature = "gui")]
    pub(crate) resize_edge_widget_hovered: bool,
    /// Core와 공유하는 프리셋 저장소 Arc. GUI 팝업이 읽으며 IPC는 Core 쪽 핸들을 사용한다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "headless receives the preset store copy but only gui popups read it"
        )
    )]
    pub(crate) preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
    /// Core와 공유하는 메모리 저장소. 화면 처리와 종료 정리에서도 사용한다.
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    /// 닫힌 surface의 lifecycle 알림 대기열. App이 꺼내 플러그인에 전송한다.
    pub(crate) pending_lifecycle_events: Vec<PendingSurfaceClosed>,
    /// 호스트 이벤트 대기열. GUI는 이벤트를 전송하고 헤드리스는 필요한 종류만 처리한다.
    pub(crate) pending_host_events: Vec<PendingHostEvent>,
    /// surface 포커스 변경을 감지할 이전 값.
    #[cfg(feature = "gui")]
    pub(crate) last_focused_surface_id: Option<u32>,
    /// 워크스페이스 전환을 감지할 이전 ID.
    #[cfg(feature = "gui")]
    pub(crate) last_active_workspace_id: Option<u32>,
    /// 탭 포커스 변경을 감지할 이전 (pane_id, tab_id).
    #[cfg(feature = "gui")]
    pub(crate) last_focused_tab: Option<(u32, u32)>,
    /// pane 간 탭 이동 감지용 tab_id → (pane_id, workspace_id, kind) 사본.
    /// 최초 조회는 비교 기준만 저장한다.
    #[cfg(feature = "gui")]
    pub(crate) last_tab_locations: Option<std::collections::HashMap<u32, (u32, u32, String)>>,

    /// Per-surface host view state for `ExplorerPanel` (directory entry cache, selection,
    /// sidebar tree expansion). `ExplorerPanel` itself only holds navigation/tab state.
    #[cfg(feature = "gui")]
    pub(crate) explorer_views: crate::adapters::ui::surface::explorer::view::ExplorerViewStore,

    /// DAG 그래프의 조회·레이아웃 캐시와 줌·이동·선택 상태.
    #[cfg(feature = "gui")]
    pub(crate) dag_graph_views: crate::adapters::ui::surface::dag_graph::DagGraphViewStore,

    /// 활성 플러그인의 도구 메뉴 항목. 매니저의 등록·활성 상태가 바뀔 때 갱신한다.
    #[cfg(feature = "gui")]
    pub(crate) tool_registry: crate::plugin::tool_registry::ToolRegistry,

    /// 팔레트에 표시할 플러그인 명령 사본. PopupDef의 draw는 매니저에 직접 접근할 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) palette_plugin_commands: Vec<crate::plugin::command_registry::PluginCommandEntry>,

    /// 팔레트에서 선택한 플러그인 명령 대기열. App이 매니저에 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_plugin_command_invokes: Vec<(String, String)>,

    /// 도구 메뉴에서 선택한 이벤트 대기열. App이 매니저에 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_tool_events: Vec<(String, serde_json::Value)>,

    /// App이 열 플러그인 팝업 요청.
    #[cfg(feature = "gui")]
    pub(crate) pending_popup_opens: Vec<PendingPopupOpen>,

    /// 파일 핸들러의 플러그인 IPC 요청 (ipc_method, target). App이 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_handler_ipc: Vec<(String, crate::file::format::FileTarget)>,

    /// 외부 파일 드래그 중 경로 목록. 취소하거나 drop하면 지운다.
    #[cfg(feature = "gui")]
    pub(crate) drop_hover: Option<DropHoverState>,

    /// drop한 파일 경로. 프레임 끝에 DispatchFile 명령으로 처리한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_file_drops: Vec<std::path::PathBuf>,

    /// 렌더 중 발견한 팝업 닫기 요청. App이 매니저에 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>,

    /// 클릭한 플러그인 팝업을 앞으로 가져올 요청. 렌더 중에는 매니저를 변경하지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_focus_bumps: Vec<u64>,

    /// TTL이나 닫기 버튼으로 종료한 배너. 렌더 이후 App이 매니저에 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_banner_closes: Vec<(u64, tasty_plugin_protocol::BannerCloseReason)>,

    /// 플러그인 팝업의 (instance_id, 콘텐츠 물리 rect). GPU가 z-order에 맞춰 mesh를 합성한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_popup_regions: Vec<(u64, crate::model::PhysicalRect)>,

    /// 키 포커스가 있는 플러그인 팝업의 IME 캐럿(창 물리 좌표).
    /// PopupPaintFrame에서 받은 값을 렌더 때 갱신하며 MainView가 OS 후보창 위치에 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_ime_cursor_area: Option<crate::model::PhysicalRect>,

    /// 플러그인 팝업별 context 전송 상태. MeshForwardCommon을 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_popup_forward:
        std::collections::HashMap<u64, crate::plugin_bridge::MeshForwardCommon>,

    /// 버튼·키 누름을 전달한 플러그인 팝업과 소유자. 팝업을 닫을 때 지운다.
    /// file_handler.dispatch가 사용자 요청으로 분류할 때 사용하는 기록이다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_user_activated: std::collections::HashMap<u64, String>,

    /// WebView별 최신 사용자 navigation 기록. 플러그인·surface·URL이 맞으면 한 번 소비한다.
    /// 기록·정리 조건은 crate::plugin_bridge::user_navigation에서 관리한다.
    #[cfg(feature = "gui")]
    pub(crate) webview_user_navigations: crate::plugin_bridge::user_navigation::UserNavigations,

    /// 플러그인 배너의 (instance_id, 콘텐츠 물리 rect). 호스트 egui 뒤에 mesh를 합성한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_banner_regions: Vec<(u64, crate::model::PhysicalRect)>,

    /// 플러그인 배너별 context 전송 상태. MeshForwardCommon을 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_banner_forward:
        std::collections::HashMap<u64, crate::plugin_bridge::MeshForwardCommon>,

    /// 크기·입력 변경 없이 다시 그려야 할 팝업 인스턴스.
    /// 플러그인 자체 요청과 원격 조회 결과 수신이 이 집합을 채운다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_popup_pending_repaint: std::collections::HashSet<u64>,

    /// 플러그인 자체 repaint 요청을 받은 배너 인스턴스.
    #[cfg(feature = "gui")]
    pub(crate) plugin_mesh_banner_pending_repaint: std::collections::HashSet<u64>,

    /// UI·도메인 Intent 대기열. 처리 규칙은 docs/design/flows/action-dispatch.md를 따른다.
    pub(crate) pending_intents: Vec<crate::intent::DispatchedIntent>,
}

/// 팝업 열기 요청. context는 플러그인에 보내고 target_surface는 호스트가 scope를 묶을 때만 쓴다.
/// 플러그인 context의 surface_id 같은 키를 호스트 소속 범위로 해석하지 않는다.
#[cfg(feature = "gui")]
#[derive(Debug, Clone)]
pub(crate) struct PendingPopupOpen {
    pub(crate) plugin_id: String,
    pub(crate) popup_id: String,
    pub(crate) context: serde_json::Value,
    pub(crate) target_surface: Option<u32>,
}

/// 파일 드래그 중 누적한 경로와 시작 좌표.
#[cfg(feature = "gui")]
#[derive(Debug, Clone, Default)]
pub struct DropHoverState {
    pub(crate) paths: Vec<std::path::PathBuf>,
    /// 드래그 중 이동 이벤트가 없을 수 있어 기록한 시작 좌표(물리 픽셀).
    #[allow(dead_code)]
    pub(crate) cursor: Option<(f32, f32)>,
}

impl AppState {
    /// 메모리 저장소를 잠그고 함수를 실행한다. poison은 Core와 같은 정책으로 복구한다.
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard = crate::poison::recover_mutex(
            self.memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
        f(&mut *guard)
    }

    /// 기존 engine의 복원된 활성 인덱스와 저장소 핸들로 화면 상태를 초기화한다.
    pub fn new(
        engine: &mut CoreState,
        preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> Self {
        let active_workspace = engine.restored_active_workspace.take().unwrap_or(0);
        Self {
            preset_store,
            memory,
            active_workspace,
            #[cfg(any(feature = "gui", debug_assertions, test))]
            category_last_active: std::collections::HashMap::new(),
            #[cfg(any(feature = "gui", debug_assertions))]
            settings_open_requested: false,
            #[cfg(any(feature = "gui", debug_assertions))]
            active_modal_id: None,
            #[cfg(any(feature = "gui", debug_assertions))]
            active_modal_kind: None,
            #[cfg(feature = "gui")]
            plugins_open: false,
            #[cfg(feature = "gui")]
            sidebar_width: engine.settings.appearance.sidebar_width,
            #[cfg(feature = "gui")]
            sidebar_visible: true,
            #[cfg(feature = "gui")]
            sidebar_collapsed: false,
            #[cfg(feature = "gui")]
            pending_resize_cursor: None,
            #[cfg(feature = "gui")]
            switch_overlay: None,
            #[cfg(feature = "gui")]
            modifier_hint: crate::adapters::ui::modifier_hint_overlay::ModifierHintRuntime::default(
            ),
            #[cfg(feature = "gui")]
            tutorial: crate::adapters::ui::tutorial::TutorialRuntime::default(),
            #[cfg(feature = "gui")]
            dialogs: DialogState::new(),
            #[cfg(any(feature = "gui", test))]
            tab_bar_height: PhysicalPx(0.0),
            pending_lifecycle_events: Vec::new(),
            pending_host_events: Vec::new(),
            #[cfg(feature = "gui")]
            last_focused_surface_id: None,
            #[cfg(feature = "gui")]
            last_active_workspace_id: None,
            #[cfg(feature = "gui")]
            last_focused_tab: None,
            #[cfg(feature = "gui")]
            last_tab_locations: None,
            #[cfg(feature = "gui")]
            popup_hovered: false,
            #[cfg(any(feature = "gui", debug_assertions))]
            plugin_popup_open: false,
            #[cfg(feature = "gui")]
            banner_hovered: false,
            #[cfg(feature = "gui")]
            modifier_hint_hovered: false,
            #[cfg(feature = "gui")]
            popup_layers: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_popup_layers: Vec::new(),
            #[cfg(feature = "gui")]
            host_popup_hittest: Vec::new(),
            #[cfg(feature = "gui")]
            popup_escape_owner: None,
            #[cfg(feature = "gui")]
            plugin_popup_hittest: Vec::new(),
            #[cfg(feature = "gui")]
            banner_layer: None,
            #[cfg(feature = "gui")]
            modifier_hint_layer: None,
            #[cfg(feature = "gui")]
            resize_edge_widget_hovered: false,
            recent_files: crate::recent_files::RecentFiles::load(),
            #[cfg(feature = "gui")]
            popups: {
                let mut pm = crate::adapters::ui::PopupManager::new();
                let ui_zoom = engine.settings.appearance.ui_scale_factor();
                for def in crate::adapters::ui::popup::defs::all_defs() {
                    pm.register_def(def, ui_zoom);
                }
                pm
            },
            #[cfg(feature = "gui")]
            fullscreen_stage: None,
            #[cfg(feature = "gui")]
            stage_closed_queue: Vec::new(),
            #[cfg(feature = "gui")]
            stage_deferred_grid_resync: false,
            #[cfg(feature = "gui")]
            search: crate::search_state::SearchState::new(),
            #[cfg(feature = "gui")]
            port_scan: crate::adapters::ui::popup::port_scanner::PortScanState::Idle,
            #[cfg(feature = "gui")]
            port_favorites_scan: crate::adapters::ui::popup::port_scanner::PortScanState::Idle,
            #[cfg(feature = "gui")]
            command_palette: crate::state::command_palette::CommandPaletteState::default(),
            #[cfg(feature = "gui")]
            toasts: crate::adapters::ui::ToastManager::new(),
            #[cfg(feature = "gui")]
            banners: crate::adapters::ui::BannerManager::new(),
            #[cfg(feature = "gui")]
            explorer_views: Default::default(),
            #[cfg(feature = "gui")]
            dag_graph_views: Default::default(),
            #[cfg(feature = "gui")]
            tool_registry: crate::plugin::tool_registry::ToolRegistry::new(),
            #[cfg(feature = "gui")]
            palette_plugin_commands: Vec::new(),
            #[cfg(feature = "gui")]
            pending_plugin_command_invokes: Vec::new(),
            #[cfg(feature = "gui")]
            pending_tool_events: Vec::new(),
            #[cfg(feature = "gui")]
            pending_popup_opens: Vec::new(),
            #[cfg(feature = "gui")]
            pending_handler_ipc: Vec::new(),
            #[cfg(feature = "gui")]
            drop_hover: None,
            #[cfg(feature = "gui")]
            pending_file_drops: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_popup_closes: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_popup_focus_bumps: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_banner_closes: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_mesh_popup_regions: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_popup_ime_cursor_area: None,
            #[cfg(feature = "gui")]
            plugin_mesh_popup_forward: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            plugin_popup_user_activated: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            webview_user_navigations: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            plugin_mesh_banner_regions: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_mesh_banner_forward: std::collections::HashMap::new(),
            #[cfg(feature = "gui")]
            plugin_mesh_popup_pending_repaint: std::collections::HashSet::new(),
            #[cfg(feature = "gui")]
            plugin_mesh_banner_pending_repaint: std::collections::HashSet::new(),
            pending_intents: Vec::new(),
        }
    }

    pub fn dispatch_intent(&mut self, intent: crate::intent::DispatchedIntent) {
        self.pending_intents.push(intent);
    }

    pub fn take_pending_intents(&mut self) -> Vec<crate::intent::DispatchedIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    /// 비어 있지 않은 file 파라미터를 kind의 최근 목록에 기록한다.
    /// 호출자가 records_recent capability를 확인해야 한다.
    pub(crate) fn record_recent(&mut self, kind: &str, params: &serde_json::Value) {
        if let Some(file) = params.get("file").and_then(|v| v.as_str())
            && !file.is_empty()
        {
            self.recent_files.add(kind, file.to_string());
        }
    }

    /// 등록된 convert_input_popup을 열도록 요청한다. 변환 대상이 있으면 context에 담는다.
    /// kind나 팝업 선언을 찾지 못하면 경고를 기록하고 false를 반환한다.
    #[cfg(feature = "gui")]
    pub(crate) fn enqueue_convert_input_popup(
        &mut self,
        engine: &CoreState,
        kind: &str,
        convert_surface_id: Option<u32>,
    ) -> bool {
        let Some(popup_ref) = engine
            .surface_registry
            .get(kind)
            .and_then(|d| d.convert_input_popup.clone())
        else {
            tracing::warn!(
                "convert-input popup: kind '{kind}' has no convert_input_popup capability"
            );
            return false;
        };
        let Some((plugin_id, local_id)) = popup_ref.split_once('/') else {
            tracing::warn!("convert-input popup: malformed convert_input_popup '{popup_ref}'");
            return false;
        };
        // 변환 대상이 있으면 포커스와 달라도 그 surface의 cwd를 사용한다.
        let origin = convert_surface_id.or_else(|| self.focused_surface_id(engine));
        let mut context = self.popup_surface_context(engine, origin);
        if let Some(sid) = convert_surface_id {
            context["surface_id"] = serde_json::json!(sid);
        }
        self.pending_popup_opens.push(PendingPopupOpen {
            plugin_id: plugin_id.to_string(),
            popup_id: local_id.to_string(),
            context,
            target_surface: origin,
        });
        true
    }

    /// 도구 메뉴·변환 팝업에 보낼 surface 컨텍스트.
    /// cwd는 inherit_cwd 설정을 따르는 로컬 경로, observed_cwd는 설정과 무관한 로컬 경로다.
    /// 원격 경로는 remote_cwd에 따로 담으며 없으면 해당 관측 키를 생략한다.
    /// origin_surface_id는 대상 ID이고, mirror이면 mirror와 local_surface_id를 추가한다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn popup_surface_context(
        &self,
        engine: &CoreState,
        surface_id: Option<u32>,
    ) -> serde_json::Value {
        use crate::core::state::SurfaceCwd;
        let Some(sid) = surface_id else {
            return serde_json::json!({ "cwd": null });
        };
        let cwd = self
            .resolve_inherit_cwd_from_surface(engine, sid)
            .map(|p| p.to_string_lossy().into_owned());
        let mut context = serde_json::json!({ "cwd": cwd, "origin_surface_id": sid });
        match engine.surface_cwd(sid) {
            Some(SurfaceCwd::Local(p)) => {
                context["observed_cwd"] = serde_json::json!(p.to_string_lossy());
            }
            Some(SurfaceCwd::Remote(r)) => {
                context["remote_cwd"] = serde_json::json!(r.as_str());
            }
            None => {}
        }
        if engine.is_mirror_surface(sid) {
            context["mirror"] = serde_json::json!(true);
            context["local_surface_id"] = serde_json::json!(sid);
        }
        context
    }

    /// 텍스트 입력 대화상자가 있는지 반환한다. 헤드리스 debug 조회에서는 false다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub fn has_input_dialog_open(&self) -> bool {
        #[cfg(feature = "gui")]
        {
            self.dialogs.has_text_input_open()
        }
        #[cfg(not(feature = "gui"))]
        {
            false
        }
    }

    /// 키·IME를 egui로 보내고 터미널 전달을 막을지 판단한다. 두 입력 경로가 같은 조건을 사용한다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) fn keyboard_overlay_open(&self) -> bool {
        #[cfg(feature = "gui")]
        let host_popup_focused = self.popups.has_focused()
            || (self.tutorial.active.is_some() && self.tutorial.keyboard_focus);
        #[cfg(not(feature = "gui"))]
        let host_popup_focused = false;
        keyboard_overlay_open(
            self.settings_open_requested,
            self.has_input_dialog_open(),
            host_popup_focused,
            self.plugin_popup_open,
        )
    }

    /// 파일 피커 요청자의 owner_popup_instance로 열린 자식이 있는지 확인한다.
    #[cfg(feature = "gui")]
    pub(crate) fn plugin_popup_has_open_child(&self, instance_id: u64) -> bool {
        self.dialogs
            .file_picker
            .as_ref()
            .and_then(|d| d.requester.as_ref())
            .and_then(|r| r.owner_popup_instance)
            == Some(instance_id)
    }

    /// 같은 화면 위의 egui 오버레이 때문에 네이티브 WebView를 가려야 하는지 판단한다.
    #[cfg(feature = "gui")]
    pub fn has_egui_overlay_open(&self) -> bool {
        // 모달은 별도 OS 창이므로 열기 요청이나 활성 모달 상태를 이 화면의 가림 조건에 넣지 않는다.
        let open = self.dialogs.has_any_overlay() || self.plugin_popup_open;
        // 무대가 열려도 OS 자식 WebView는 그대로 남으므로 표시를 명시적으로 꺼야 한다.
        open || self.popups.has_visible_open()
            || self.fullscreen_stage.is_some()
            || self.tutorial.active.is_some()
    }

    /// 같은 무대는 유지하고 다른 무대는 닫은 뒤 교체한다. 정의에 없는 ID는 false를 반환한다.
    /// 호출자는 화면을 다시 그리도록 dirty를 설정해야 한다.
    #[cfg(feature = "gui")]
    pub fn open_fullscreen_stage(&mut self, id: &str) -> bool {
        let Some(def) = crate::adapters::ui::fullscreen::defs::find(id) else {
            return false;
        };
        if self
            .fullscreen_stage
            .as_ref()
            .is_some_and(|s| s.id == def.id())
        {
            return true;
        }
        self.close_fullscreen_stage();
        self.fullscreen_stage = Some(crate::adapters::ui::fullscreen::StageState { id: def.id() });
        true
    }

    /// 활성 무대를 제거하고 on_close 처리 큐에 넣는다. 활성 무대가 없으면 false다.
    #[cfg(feature = "gui")]
    pub fn close_fullscreen_stage(&mut self) -> bool {
        match self.fullscreen_stage.take() {
            Some(stage) => {
                self.stage_closed_queue.push(stage.id);
                true
            }
            None => false,
        }
    }

    #[cfg(feature = "gui")]
    pub fn fullscreen_stage_id(&self) -> Option<crate::adapters::ui::fullscreen::StageId> {
        self.fullscreen_stage.as_ref().map(|s| s.id)
    }

    /// 전체화면 무대 표시 여부. 헤드리스 debug 조회에서는 false다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub fn fullscreen_stage_active(&self) -> bool {
        #[cfg(feature = "gui")]
        {
            self.fullscreen_stage.is_some()
        }
        #[cfg(not(feature = "gui"))]
        {
            false
        }
    }

    /// 닫힌 surface의 터미널·화면 상태·메모리 정리를 요청한다.
    /// persist_id가 있으면 해당 스크롤백 파일 삭제도 시도한다.
    pub(crate) fn cleanup_surface(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
    ) {
        let mut sink = crate::close_trace::CleanupSums::default();
        self.cleanup_surface_traced(engine, surface_id, persist_id, &mut sink);
    }

    /// cleanup_surface와 같은 정리를 하며 단계별 시간을 sums에 합산한다.
    pub(crate) fn cleanup_surface_traced(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    ) {
        use std::time::Instant;
        sums.surfaces += 1;
        let t = Instant::now();
        Self::delete_scrollback_persist(persist_id);
        sums.scrollback_delete += t.elapsed();
        let t = Instant::now();
        self.drop_terminal(engine, surface_id);
        sums.terminal_drop += t.elapsed();
        let t = Instant::now();
        self.drop_surface_indices(engine, surface_id);
        sums.indices_drop += t.elapsed();
        let t = Instant::now();
        self.purge_surface_memory_scope(surface_id);
        sums.memory_purge += t.elapsed();
        // 닫힌 surface 점유만 지운다. 다른 surface가 남은 workspace 점유는 유지한다.
        engine.attach.forget_closed_surface(surface_id);
    }

    fn delete_scrollback_persist(persist_id: Option<String>) {
        if let Some(pid) = persist_id {
            crate::scrollback_store::delete(&pid);
        }
    }

    /// Terminal과 부속 상태를 저장소에서 제거한다. 실제 종료 처리는 Terminal의 Drop에 맡긴다.
    fn drop_terminal(&mut self, engine: &mut CoreState, surface_id: u32) {
        engine.pending_scrollback_inject.remove(&surface_id);
        if let Some(old_terminal) = engine.terminals.remove(surface_id) {
            drop(old_terminal);
        }
    }

    fn drop_surface_indices(&mut self, engine: &mut CoreState, surface_id: u32) {
        #[cfg(feature = "gui")]
        {
            self.explorer_views.drop_view(surface_id);
            self.dag_graph_views.drop_view(surface_id);
        }
        engine.command_index.drop_surface(surface_id);
        engine.observer_router.drop_surface(surface_id);
        engine.hook_manager.remove_surface_hooks(surface_id);
        engine.forget_shell_integration_hint(surface_id);
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(surface_id);
        }
    }

    /// surface 범위의 regular·secret 메모리를 삭제한다.
    /// 메타데이터뿐 아니라 플러그인·Lua가 직접 저장한 키도 포함한다.
    fn purge_surface_memory_scope(&mut self, surface_id: u32) {
        let scope = tasty_memory::Scope::Surface(surface_id);
        match self.with_memory(|m| m.purge_scope(&scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                surface_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-surface scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(surface_id, "memory: purge_scope failed: {e}"),
        }
    }

    /// 워크스페이스 복원 사본을 만든다. 저장 여부는 호출자가 결정한다.
    fn capture_workspace_snapshot(engine: &CoreState, ws_idx: usize) -> crate::model::ClosedItem {
        let mut snap_fn = crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
        let ws = &engine.workspaces[ws_idx];
        let terminals = &engine.terminals;
        crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
    }

    /// 제거 전에 모든 surface의 ID와 스크롤백 저장 ID를 수집한다.
    fn collect_workspace_close_targets(
        engine: &CoreState,
        ws_idx: usize,
    ) -> Vec<(u32, Option<String>)> {
        let mut targets = Vec::new();
        let ws = &engine.workspaces[ws_idx];
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    crate::core::impl_close::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }
        targets
    }

    /// 워크스페이스를 제거한 뒤 닫힘 이벤트를 큐에 넣고 메모리 정리를 시도한다.
    /// path는 종료 시간 로그의 경로 구분값이다.
    pub(crate) fn after_workspace_removed(&mut self, workspace_id: u32, path: &'static str) {
        self.enqueue_host_event(PendingHostEvent::WorkspaceClosed { workspace_id });
        let t = std::time::Instant::now();
        self.purge_workspace_memory_scope(workspace_id);
        crate::close_trace::log_ws_purge(t, path);
    }

    /// 워크스페이스 범위 메모리를 정리한다. 이벤트 기록과 함께 실행하도록 after_workspace_removed를 쓴다.
    fn purge_workspace_memory_scope(&mut self, workspace_id: u32) {
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        match self.with_memory(|m| m.purge_scope(&ws_scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        }
    }

    /// 수집한 대상을 정리하고 lifecycle 알림을 큐에 넣는다.
    /// 제거 후에는 kind를 찾을 수 없으므로 호출자가 넘긴 값을 사용한다.
    fn cleanup_targets(
        &mut self,
        engine: &mut CoreState,
        targets: Vec<(u32, Option<String>, Option<&'static str>)>,
        is_user_close: bool,
        trace: Option<&'static str>,
    ) {
        let t_loop = std::time::Instant::now();
        let mut sums = crate::close_trace::CleanupSums::default();
        for (sid, pid, kind) in targets {
            self.cleanup_surface_traced(engine, sid, pid, &mut sums);
            self.enqueue_surface_closed(sid, kind, is_user_close);
        }
        if let Some(path) = trace {
            sums.log(t_loop.elapsed(), path);
        }
    }

    #[cfg(feature = "gui")]
    pub fn focused_surface_type(&self, engine: &CoreState) -> FocusedSurfaceType {
        let pane = match self.focused_pane(engine) {
            Some(p) => p,
            None => return FocusedSurfaceType::None,
        };
        let tab = match pane.tabs.get(pane.active_tab) {
            Some(t) => t,
            None => return FocusedSurfaceType::None,
        };

        if let Some(leaf) = tab.layout().find_surface(tab.focused_surface) {
            return Self::surface_to_type(leaf);
        }

        FocusedSurfaceType::None
    }

    #[cfg(feature = "gui")]
    fn surface_to_type(surface: &dyn crate::model::Surface) -> FocusedSurfaceType {
        match surface.kind() {
            "terminal" => FocusedSurfaceType::Terminal,
            other => FocusedSurfaceType::Kind(other.to_string()),
        }
    }

    pub fn surface_kind(&self, engine: &CoreState, surface_id: u32) -> Option<&'static str> {
        engine.find_surface_by_id(surface_id).map(|s| s.kind())
    }

    pub fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    ) {
        self.pending_lifecycle_events.push(PendingSurfaceClosed {
            surface_id,
            kind,
            is_user_close,
        });
    }

    #[cfg(any(feature = "gui", test))]
    pub fn take_pending_lifecycle_events(&mut self) -> Vec<PendingSurfaceClosed> {
        std::mem::take(&mut self.pending_lifecycle_events)
    }

    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.pending_host_events.push(event);
    }

    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.pending_host_events)
    }

    /// 이미 알린 탭 변경을 다음 폴링에서 중복 보고하지 않도록 기준 사본을 갱신한다.
    /// 최초 폴링 전이면 그 폴링이 기준을 만들도록 그대로 둔다.
    #[cfg(feature = "gui")]
    pub fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    ) {
        if let Some(map) = self.last_tab_locations.as_mut() {
            map.insert(tab_id, (pane_id, workspace_id, kind));
        }
    }

    #[cfg(feature = "gui")]
    pub fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32) {
        if let Some(map) = self.last_tab_locations.as_mut() {
            map.remove(&tab_id);
        }
    }

    /// cwd 상속 설정이 켜져 있으면 포커스된 surface의 로컬 경로를 반환한다.
    /// 원격 mirror의 경로는 로컬 PTY 작업 디렉터리로 사용할 수 없어 제외한다.
    pub(crate) fn resolve_inherit_cwd(&self, engine: &CoreState) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd || engine.workspaces.is_empty() {
            return None;
        }
        let sid = self.focused_surface_id(engine)?;
        engine.local_surface_cwd(sid)
    }

    /// cwd 상속 설정이 켜져 있으면 지정한 surface의 로컬 경로를 반환한다.
    pub(crate) fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd {
            return None;
        }
        engine.local_surface_cwd(surface_id)
    }
}

/// 플러그인 팝업도 egui 입력이 필요하지만 호스트 PopupManager에는 없으므로 별도로 검사한다.
#[cfg(any(feature = "gui", debug_assertions, test))]
pub(crate) fn keyboard_overlay_open(
    settings_open_requested: bool,
    input_dialog_open: bool,
    host_popup_focused: bool,
    plugin_popup_open: bool,
) -> bool {
    settings_open_requested || input_dialog_open || host_popup_focused || plugin_popup_open
}

#[cfg(test)]
mod keyboard_overlay_tests {
    use super::keyboard_overlay_open;

    #[test]
    fn plugin_popup_alone_opens_the_gate() {
        assert!(keyboard_overlay_open(false, false, false, true));
    }

    #[test]
    fn nothing_open_keeps_the_gate_closed() {
        assert!(!keyboard_overlay_open(false, false, false, false));
    }

    #[test]
    fn each_existing_predicate_still_opens_the_gate() {
        assert!(keyboard_overlay_open(true, false, false, false));
        assert!(keyboard_overlay_open(false, true, false, false));
        assert!(keyboard_overlay_open(false, false, true, false));
    }
}

#[cfg(test)]
mod tab_bar_height_seed_tests {
    use super::*;

    // 물리 높이의 미측정 초기값을 논리 토큰과 혼동하지 않아야 한다.
    #[test]
    fn the_seed_is_not_the_logical_token() {
        let (state, _engine) = super::tests::test_state();
        let seeded = state.tab_bar_height;
        assert_eq!(seeded, PhysicalPx(0.0), "측정 전 탭 바 높이는 0이어야 한다");
        assert_ne!(
            seeded.value(),
            tasty_type_appearance::theme::SIZING.tab_bar_height.value(),
            "측정 전 물리 높이에 논리 길이 토큰을 넣으면 안 된다"
        );
    }
}
