//! Main View presentation and application request state.

use super::*;

pub struct MainViewState {
    pub(crate) terminal_views: terminal_view::TerminalViewports,
    pub(crate) navigation: navigation::NavigationState,
    pub(crate) layout_previews: layout_preview::LayoutPreviews,
    /// Tab bar viewport by stable pane ID; never part of the domain layout.
    #[cfg(feature = "gui")]
    pub(crate) tab_bar_scroll: std::collections::HashMap<u32, LogicalPx>,
    /// 카테고리별로 마지막에 선택한 워크스페이스 ID. 영속화하지 않는다.
    /// 재정렬에 영향을 받지 않도록 인덱스 대신 ID를 저장하며, 찾지 못하면 첫 항목을 선택한다.
    #[cfg(any(feature = "gui", debug_assertions, test))]
    pub(crate) category_last_active: std::collections::HashMap<
        tasty_utils::id::WorkspaceCategoryId,
        tasty_utils::id::WorkspaceId,
    >,
    /// 설정 모달 열기 요청. 화면에 이미 표시됐는지는 ViewRegistry의 활성 모달로 확인한다.
    /// redraw의 dispatch_pending_modal_opens 또는 Escape 처리가 요청을 지운다.
    // release 헤드리스에는 이 요청을 읽는 경로가 없다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub(crate) settings_open_requested: bool,
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
    /// 상태바에 표시할 선택 surface의 Git branch 캐시. 무효화는 branch 모듈이 맡는다.
    #[cfg(feature = "gui")]
    pub(crate) branch_cache: branch::BranchCache,
    /// 이 창에서 셸 통합 안내 배너를 이미 띄운 surface. 안내 요청을 한 번만 보내는 판단은 Core가 한다.
    #[cfg(feature = "gui")]
    pub(crate) shell_integration_hint_shown: std::collections::HashSet<u32>,
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

    /// Explorer의 파일 복사·잘라내기 목록. 창마다 하나이며 OS 텍스트 클립보드와 별개다. 저장하지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) explorer_clipboard: Option<ExplorerClipboard>,

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

    /// 사용자가 선택한 파일 처리기 이력. 창을 만들 때 공용 파일에서 읽고 선택할 때마다 저장한다.
    #[cfg(feature = "gui")]
    pub(crate) file_handler_recent: crate::file::handler::recent::RecentPicks,

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

    /// debug IPC가 요청한 webview 탐색 조작. sync_webviews가 소비한다.
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) debug_webview_history: Vec<(u32, crate::webview::DebugHistoryAction)>,

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

    /// html surface마다 스크립트 배너가 차지하는 높이. 패널 위쪽에서 페이지가 시작할 곳까지다.
    /// egui 패스가 다시 채우고 WebView 동기화가 이만큼 native 페이지를 내린다(ADR-0053).
    #[cfg(feature = "gui")]
    pub(crate) html_script_banner_insets:
        std::collections::HashMap<u32, tasty_type_geometry::length::LogicalPx>,

    /// UI·도메인 Intent 대기열. 처리 규칙은 docs/design/flows/action-dispatch.md를 따른다.
    pub(crate) pending_intents: Vec<crate::intent::DispatchedIntent>,
}
