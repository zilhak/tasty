// headless 빌드에선 호출 트리 (app::dispatch/intents) 가 cfg(gui) 로 가려져
// state 의 gui 전용 필드/메서드가 미사용으로 잡힌다. 본질적으로 gui 어댑터의
// API 면이므로 *headless 한정* 으로 dead_code/unused_imports 를 침묵시킨다.
// gui 빌드에서는 검사 그대로 작동.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

mod accessors;
mod detect;
mod focus;
mod layout;
mod mark;
mod mouse;
pub(crate) mod pane;
mod tab;
#[cfg(test)]
mod tests;
mod workspace;

pub mod command_palette;
pub mod preset_apply;
pub mod search;
pub mod selection;

use std::collections::VecDeque;

#[cfg(feature = "gui")]
use crate::adapters::ui::info_modal::InfoModal;
use crate::core::CoreState;
use crate::model::{LogicalPx, PhysicalPx};
#[cfg(feature = "gui")]
use crate::settings_ui::SettingsUiState;

/// Type of the currently focused surface, used for keyboard routing.
///
/// Terminal은 PTY 입출력 경로가 별도라 빠른 분기 위해 전용 variant로 둔다.
/// 나머지는 surface kind 식별자 기반의 `Kind(String)`으로 일반화 — 외부 plugin도
/// 추가 enum 변경 없이 동작한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedSurfaceType {
    None,
    Terminal,
    Kind(String),
}

impl FocusedSurfaceType {
    /// 이 surface가 주어진 kind 식별자에 해당하는지 검사.
    pub fn is_kind(&self, kind: &str) -> bool {
        matches!(self, Self::Kind(k) if k == kind)
    }
}

/// A keyboard event destined for a non-terminal surface (Explorer, Markdown, etc.).
/// Stored in a queue and consumed during the next egui render frame.
#[cfg(feature = "gui")]
#[derive(Debug, Clone)]
pub struct PendingKeyEvent {
    pub(crate) key: winit::keyboard::Key,
}

#[derive(Debug, Clone)]
pub struct SurfaceMessage {
    pub(crate) id: u32,
    pub(crate) from_surface_id: u32,
    pub(crate) content: String,
}

/// Surface가 닫혔다는 사실을 plugin 측에 broadcast하기 위해 메인 루프가 소비할
/// 큐 항목. `state/`는 `plugin/` 의존이 없으므로 enum 대신 `is_user_close: bool`로
/// reason을 담고, App 메인 루프에서 `SurfaceCloseReason`으로 매핑한다.
#[derive(Debug, Clone)]
pub struct PendingSurfaceClosed {
    pub(crate) surface_id: u32,
    /// kind 가 None 인 경우는 cascade close 경로에서 surface 가 이미 layout 에서
    /// 제거된 뒤 enqueue 되어 식별이 불가능했음을 의미. payload 변환 시 빈 문자열로
    /// 폴백한다 — 구독자(예: plugin-claude)는 surface_id 만으로 cleanup 가능.
    pub(crate) kind: Option<&'static str>,
    pub(crate) is_user_close: bool,
}

/// Event Bus 1.0 호스트 자동 발화용 큐 항목. `state/`가 `plugin/`/`tasty-plugin-protocol`
/// 의존을 갖지 않게, payload 필드는 wire 타입이 아닌 plain 데이터로 보관하고 App
/// 메인 루프가 [`tasty_plugin_protocol`] 타입으로 변환해 발화한다.
#[derive(Debug, Clone)]
pub enum PendingHostEvent {
    SurfaceFocused {
        surface_id: u32,
        prev_surface_id: Option<u32>,
    },
    SurfaceTitleChanged {
        surface_id: u32,
        title: String,
    },
    SurfaceCreated {
        surface_id: u32,
        kind: &'static str,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        /// `None`이면 user-initiated, `Some(plugin_id)`면 agent(plugin)이 spawn한 결과.
        created_by_plugin: Option<String>,
    },
    WorkspaceActivated {
        workspace_id: u32,
        prev_workspace_id: Option<u32>,
    },
    /// 이름/부제/설명 중 변경된 필드만 `Some`. 호스트 발화 측 어디서나 partial
    /// update가 가능하도록 모두 Optional로 둔다.
    ///
    /// `user_direct=true`면 사용자가 GUI 다이얼로그로 직접 변경한 케이스. Lua
    /// hook 의 `workspace.change.post` 는 user_direct 만 발화한다 (observe-only
    /// 단계 명세). IPC/CLI 경유 변경은 false 로 들어와 plugin 이벤트 버스만 받는다.
    WorkspaceRenamed {
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
        user_direct: bool,
    },
    TabFocused {
        tab_id: u32,
        pane_id: u32,
        prev_tab_id: Option<u32>,
    },
    /// Tab 이름 변경. `user_direct=true`면 사용자가 GUI 다이얼로그로 직접 rename
    /// 한 케이스 — Lua `tab.change.post` hook 은 user_direct 만 발화한다.
    TabRenamed {
        tab_id: u32,
        title: String,
        user_direct: bool,
    },
    /// 자식 프로세스 종료. exit_code는 현재 terminal 이벤트가 노출하지 않아 `None` 고정.
    ProcessExited {
        surface_id: u32,
    },
    /// `NotificationStore::add` 결과. source는 발화 측에서 채워 push (host=`"host"`,
    /// plugin=plugin_id).
    NotificationCreated {
        id: u64,
        title: String,
        body: String,
        source: String,
    },
    /// Tab 생성. `detect_tab_lifecycle` polling으로 발견.
    TabCreated {
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    },
    /// Tab 종료. polling이 사라진 tab_id를 발견하면 마지막 위치로 enqueue.
    /// 현재 reason은 항상 User (PR 5의 caller context 도입 이후 Ipc 구분 예정).
    TabClosed {
        tab_id: u32,
        pane_id: u32,
    },
    /// Tab이 다른 pane으로 이동. polling diff로 감지.
    TabMoved {
        tab_id: u32,
        from_pane: u32,
        to_pane: u32,
    },
    /// Pane 생성. polling으로 감지. `parent_pane_group`은 트리 구조 노출 비용이
    /// 커 현재 `None` 고정 (필요해지면 PR 5 이후 확장).
    PaneCreated {
        pane_id: u32,
        workspace_id: u32,
    },
    /// Pane 종료. polling이 사라진 pane_id를 발견하면 발화.
    /// reason은 현재 항상 `User` (caller context 구분은 PR 5에서).
    PaneClosed {
        pane_id: u32,
    },
    /// Workspace 생성. polling으로 감지. `window_id`는 caller가 전달.
    WorkspaceCreated {
        workspace_id: u32,
        window_id: u64,
        name: String,
    },
    /// Workspace 종료. reason은 현재 항상 `User`.
    WorkspaceClosed {
        workspace_id: u32,
    },
    /// Pane 분할. polling으로는 direction을 알 수 없어 호출 사이트에서 직접 enqueue.
    PaneSplit {
        original_pane: u32,
        new_pane: u32,
        direction: crate::model::SplitDirection,
    },
    /// `tasty-hooks`의 surface hook 발화. `check_and_fire` 호출자가 fired hook_id
    /// 리스트와 매칭된 event를 묶어 enqueue. surface_id가 0이면 global hook.
    HookFired {
        hook_id: u64,
        event_kind: String,
        surface_id: u32,
    },
    /// 임의의 host event 발화. shortcut 핸들러처럼 plugin_manager에 직접 접근 못
    /// 하는 callsite에서 host event를 발화할 때 사용. `EventScope::System` 고정.
    Raw {
        key: String,
        payload: serde_json::Value,
    },

    // ─── Plugin lifecycle (D.3.C.G.2) ───
    /// Plugin spawn 성공 후 hello 까지 완료.
    PluginLoaded {
        plugin_id: String,
        version: String,
    },
    /// Plugin 활성화 상태 변경 (enable=true / disable=false).
    PluginEnableToggled {
        plugin_id: String,
        enabled: bool,
    },
    /// Plugin process 가 종료됨. `reason` 은 LifecycleReason 의 serde rename
    /// (snake_case) — "user" / "ipc" / "crash".
    PluginUnloaded {
        plugin_id: String,
        reason: String,
    },
    /// Plugin spawn 실패 또는 runtime error.
    PluginError {
        plugin_id: String,
        error_kind: String,
        message: String,
    },
    /// Plugin install / remove / grant / revoke 완료. `change_kind` 는
    /// "installed" / "removed" / "permission_granted" / "permission_revoked".
    PluginRegistryChanged {
        plugin_id: String,
        change_kind: String,
        detail: serde_json::Value,
    },
    /// Plugin 의 surface_kind 가 hello 처리 직후 registry 에 등록됨.
    PluginSurfaceKindRegistered {
        plugin_id: String,
        kind: String,
        rendering: String,
    },
    /// Plugin manifest 의 `[[contributes.window]]` 항목이 hello 시점에 등록됨.
    /// 1.0 schema-only — host event `plugin.window_declared` 로 가시화.
    PluginWindowDeclared {
        plugin_id: String,
        window_id: String,
    },
}

// IdGenerator is now in core_state.rs

pub struct AppState {
    // ── Window-level UI state ──
    pub(crate) active_workspace: usize,
    /// Whether the settings window is open.
    pub(crate) settings_open: bool,
    /// Whether the plugins window is open.
    pub(crate) plugins_open: bool,
    /// Persistent UI state for the settings window.
    #[cfg(feature = "gui")]
    pub(crate) settings_ui_state: SettingsUiState,
    /// Cached sidebar width from settings (logical pixels).
    pub(crate) sidebar_width: LogicalPx,
    /// Sidebar visibility: false = completely hidden.
    pub(crate) sidebar_visible: bool,
    /// Sidebar collapsed: true = compact mode (narrow width, icons only).
    pub(crate) sidebar_collapsed: bool,
    /// 통합 리사이즈 커서 피드백 — `handle_cursor_moved` 가 창 가장자리 hover 를
    /// 감지하면 그 8방향을 저장하고, egui 프레임(`run_egui_frame`)이 매 프레임
    /// `set_cursor_icon` 으로 적용한다. egui 가 winit 커서를 매 프레임 덮으므로
    /// 프레임 내에서만 적용할 수 있어 상태로 보관한다. 데코 없는 Windows/Linux
    /// 창에서만 채워진다(macOS 는 네이티브 데코라 항상 None). 콘텐츠/오버레이 위에서는
    /// None 으로 리셋된다(콘텐츠 우선 입력모델).
    #[cfg(feature = "gui")]
    pub(crate) pending_resize_cursor: Option<winit::window::ResizeDirection>,
    /// switch-number overlay 활성 스냅샷. 현재 눌린 modifier 가 tab/workspace 전환
    /// 단축키와 일치하면 그 대상(+Tab 이면 focused pane id)을 담는다. `MainView` 의
    /// `ModifiersChanged` 가 [`crate::adapters::ui::switch_overlay::switch_target_for`]
    /// 로 갱신하고, 창 비활성/포커스 상실 시 `None` 으로 clear 된다. draw 경로(04 탭
    /// /05 사이드바)가 매 프레임 읽어 숫자 키캡 오버레이를 표시할지 결정한다.
    #[cfg(feature = "gui")]
    pub(crate) switch_overlay: Option<crate::adapters::ui::switch_overlay::SwitchOverlayState>,
    /// All transient dialog/popup state.
    pub(crate) dialogs: DialogState,
    /// Measured tab bar height in physical pixels, updated each frame by egui.
    pub(crate) tab_bar_height: PhysicalPx,
    /// Popup manager for internal popups (notification panel, etc.).
    #[cfg(feature = "gui")]
    pub(crate) popups: crate::adapters::ui::PopupManager,
    /// Terminal text search state.
    pub(crate) search: crate::search_state::SearchState,
    /// Listening-port scanner async state machine. Driven by the port scanner
    /// popup: Idle → Loading (background thread + mpsc channel) → Ready / Failed.
    /// Reset to `Idle` when the popup closes.
    #[cfg(feature = "gui")]
    pub(crate) port_scan: crate::adapters::ui::popup::port_scanner::PortScanState,
    /// Command palette UI state — query buffer, selection cursor, and a pending
    /// dispatch slot that MainView drains each frame.
    pub(crate) command_palette: crate::state::command_palette::CommandPaletteState,
    /// Toast manager for transient in-app notifications (copy feedback, etc.).
    /// 사용자 행동에서만 발사한다. CLI/IPC 경유 동작은 토스트를 만들지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) toasts: crate::adapters::ui::ToastManager,
    /// Banner manager — 4번째 오버레이(공지+action). Toast/Popup 과 별도 매니저.
    /// 사용자 행동에서만 발사한다. IPC/release cascade 는 배너를 띄울 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) banners: crate::adapters::ui::BannerManager,
    /// Cached recent files list (markdown/html open popups). Loaded from disk at
    /// startup and mutated in-place; each mutation saves back to disk.
    pub(crate) recent_files: crate::recent_files::RecentFiles,
    /// Whether the mouse is currently over an open popup (input layer state).
    /// Updated each frame by PopupManager::draw(). Mouse handlers check this
    /// to block events from reaching lower layers (terminal, dividers).
    pub(crate) popup_hovered: bool,
    /// Whether the mouse is currently over a banner (input layer state).
    /// Updated each frame by BannerManager::draw(). 배너는 자기 영역의 마우스를
    /// 소비(뒤로 전파 X)하므로 mouse 핸들러가 이 값으로 하위 레이어 전파를 막는다.
    /// (포커스는 받지 않음 — 마우스 소비만.) popup_hovered 와 동일하게 비-gui 빌드도
    /// 필드를 갖는다(입력 가드가 공유).
    pub(crate) banner_hovered: bool,
    /// Preset store 의 Arc clone — Core 가 owner. UI popup 이 draw 흐름에서
    /// core 인자 없이 lock 으로 read 할 수 있도록 AppState 에 *clone 보유* 만
    /// 한다 (allocation 동일, owner 는 Core). `create_app_state` 가 inject.
    pub(crate) preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
    /// Memory store 의 Arc clone — Core 가 owner. UI thread (popup draw_fn) 와
    /// engine state cleanup 이 dispatcher cascade 없이 직접 영속할 때 사용한다.
    /// `Core::with_memory` 와 같은 lock 정책 (poisoning 시 inner 사용).
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    /// Double-tap modifier captured from winit events, for the keybinding recorder to consume.
    pub(crate) captured_double_tap: Option<String>,
    /// Keyboard events for non-terminal surfaces, consumed during egui rendering.
    #[cfg(feature = "gui")]
    pub(crate) pending_surface_keys: Vec<PendingKeyEvent>,
    /// Surface close lifecycle 알림 큐. close 직후 enqueue되고, App 메인 루프가
    /// drain하여 `PluginManager::notify_surface_closed`로 dispatch한다.
    /// `state/`는 `plugin/` 의존이 없어 별도 plain struct로 둔다.
    pub(crate) pending_lifecycle_events: Vec<PendingSurfaceClosed>,
    /// Event Bus 1.0 호스트 자동 발화 큐. 호스트 코드 곳곳에서 `enqueue_host_event`로
    /// push하고, App 메인 루프가 drain해 wire payload로 변환·발화한다.
    pub(crate) pending_host_events: Vec<PendingHostEvent>,
    /// `surface.focused` 발화용 변화 감지 상태. tick마다 `focused_surface_id()`와
    /// 비교해 달라졌으면 `SurfaceFocused`를 enqueue한다. focus 전환 경로가 많아
    /// (키보드/마우스/IPC/탭전환/워크스페이스전환) 각각을 hook하기보다 polling이 단순.
    pub(crate) last_focused_surface_id: Option<u32>,
    /// `workspace.activated` 발화용 변화 감지 상태. `active_workspace` 인덱스가
    /// 가리키는 워크스페이스 ID를 기록해 두고, 다음 tick에서 달라졌다면
    /// `WorkspaceActivated`를 enqueue한다.
    pub(crate) last_active_workspace_id: Option<u32>,
    /// `tab.focused` 발화용 변화 감지 상태. 활성 워크스페이스의 focused pane이 보유한
    /// 현재 active tab의 (pane_id, tab_id)를 기록. 다음 tick에서 달라졌다면
    /// `TabFocused`를 enqueue한다. pane 전환·in-pane tab 전환을 한꺼번에 다룬다.
    pub(crate) last_focused_tab: Option<(u32, u32)>,
    /// `tab.created`/`tab.closed`/`tab.moved` 발화용 polling 상태. tab_id →
    /// (pane_id, workspace_id, kind) 스냅샷. `None`은 아직 한 번도 polling하지
    /// 않은 상태(초기 로드된 탭에 대해 spurious `tab.created`가 발화되는 것을 막기
    /// 위해 첫 호출에서는 스냅샷만 만들고 이벤트를 enqueue하지 않는다).
    pub(crate) last_tab_locations: Option<std::collections::HashMap<u32, (u32, u32, String)>>,

    /// Per-surface host view state for `MarkdownPanel` (content cache, scroll, load error).
    /// `MarkdownPanel` itself only holds `file_path` + reload tracking; everything GUI-bound lives here.
    #[cfg(feature = "gui")]
    pub(crate) markdown_views: crate::adapters::ui::surface::markdown::view::MarkdownViewStore,

    /// Per-surface host view state for `ImagePanel` (pixel buffer, textures, edit state,
    /// undo history, brush settings, popup buffers).
    #[cfg(feature = "gui")]
    pub(crate) image_views: crate::adapters::ui::surface::image::view::ImageViewStore,

    /// Per-surface host view state for `ExplorerPanel` (directory entry cache, selection,
    /// sidebar tree expansion). `ExplorerPanel` itself only holds navigation/tab state.
    #[cfg(feature = "gui")]
    pub(crate) explorer_views: crate::adapters::ui::surface::explorer::view::ExplorerViewStore,

    /// 사이드바 도구 메뉴 항목. 활성 plugin의 `[[contributes.tool]]`
    /// 항목을 합쳐 관리. PluginManager가 plugin 라이프사이클 변경 시
    /// `set_plugin_items(mgr.plugin_tool_items())`로 갱신한다.
    pub(crate) tool_registry: crate::plugin::tool_registry::ToolRegistry,

    /// 도구 메뉴 항목 클릭 시 publish해야 할 이벤트 큐. tools_menu가 `&mut AppState`만
    /// 가지므로 PluginManager에 직접 접근할 수 없어, 클릭 시점에 enqueue하고 App 메인
    /// 루프가 drain해 `PluginManager::emit_host_event`로 발화한다.
    pub(crate) pending_tool_events: Vec<(String, serde_json::Value)>,

    /// `ToolAction::OpenPopup` 클릭 시 열어야 할 popup 큐.
    /// (plugin_id, popup_id, context). App 메인 루프가 drain해
    /// `PluginManager::open_popup_instance`로 dispatch.
    pub(crate) pending_popup_opens: Vec<(String, String, serde_json::Value)>,

    /// file_handler 디스패치 결과가 plugin IPC method 일 때의 호출 큐.
    /// `(ipc_method, target)`. App 메인 루프가 drain 해 `PluginManager` 로 forward.
    pub(crate) pending_handler_ipc: Vec<(String, crate::file::format::FileTarget)>,

    /// 외부 drag&drop 으로 파일이 hover 중인 상태. `HoveredFile` 마다 path 누적,
    /// `HoveredFileCancelled` / `DroppedFile` 시 해제. 비주얼 overlay 의 입력.
    pub(crate) drop_hover: Option<DropHoverState>,

    /// `DroppedFile` 이벤트로 받은 경로 큐. frame end 에서 drain 해
    /// `DomainIntent::DispatchFile` 으로 발화.
    pub(crate) pending_file_drops: Vec<std::path::PathBuf>,

    /// plugin popup 렌더 중 수집된 사용자 입력. App 메인 루프가 drain해
    /// `PluginManager::send_popup_event`로 forward한다.
    pub(crate) plugin_popup_events: Vec<(u64, tasty_plugin_protocol::ui_tree::UiEvent)>,

    /// plugin popup 렌더 중 감지된 close 사유 (outside-click / Escape).
    /// App 메인 루프가 drain해 `PluginManager::close_popup_instance`를 호출한다.
    pub(crate) plugin_popup_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>,

    /// egui-mesh popup(A2) 합성 영역. `draw_plugin_popups` 가 매 egui frame 채우고,
    /// `gpu.render` 가 host egui pass *후* 각 (instance_id, 물리 콘텐츠 rect)에 plugin
    /// mesh 를 합성한다. 셸(scrim/bg/border)은 host egui 가, 내용만 plugin mesh 가 그린다.
    pub(crate) plugin_mesh_popup_regions: Vec<(u64, crate::model::PhysicalRect)>,

    /// egui-mesh popup 별 마지막으로 보낸 set_context geom `(w_px, h_px, ppp_bits)`.
    /// 정적 화면을 매 frame 무조건 보내지 않기 위한 변경 감지(surface forward 와 동형).
    pub(crate) plugin_mesh_popup_geom: std::collections::HashMap<u64, (u32, u32, u32)>,

    /// 이미 bootstrap set_context 를 보낸 egui-mesh popup 인스턴스. paint frame 이
    /// 아직 안 온 동안 set_context 를 1회만 보내기 위한 가드(surface `bootstrap_sent` 동형).
    /// frame 이 보이면 해제돼 crash 후 재bootstrap 된다. 핵심: 첫 frame(폰트 atlas 동봉)
    /// 을 host 가 반드시 decode 하도록, bootstrap 을 매 frame 스팸하지 않는다.
    pub(crate) plugin_mesh_popup_bootstrapped: std::collections::HashSet<u64>,

    /// egui-mesh popup 별 마지막으로 보낸 Theme 스냅샷. 크기/입력 무변이어도 테마가
    /// 바뀌면 set_context 재forward 를 트리거한다(surface `last_theme` 동형).
    pub(crate) plugin_mesh_popup_theme:
        std::collections::HashMap<u64, tasty_plugin_protocol::ThemeWire>,

    /// 호스트 내부 Intent 큐. 발화자가 push 만 하고, `App::dispatch_pending_intents`
    /// 가 메인 루프에서 drain 한다. UI Intent (`Intent::Ui`) 와 Domain Intent
    /// (`Intent::Domain`) 가 한 큐 위에서 처리됨 (D.3.I.3 통합). 설계:
    /// `docs/design/flows/action-dispatch.md`, `intent-ui-vs-domain.md`.
    pub(crate) pending_intents: Vec<crate::intent::DispatchedIntent>,
}

/// A pending native context menu request.
#[derive(Clone)]
pub enum PendingNativeMenu {
    /// Tab right-click: Rename / Close
    Tab {
        pane_id: u32,
        tab_index: usize,
        x: f32,
        y: f32,
    },
    /// Pane/empty area right-click: Open Markdown... / Open Explorer / Open HTML...
    Pane { pane_id: u32, x: f32, y: f32 },
    /// Workspace right-click in sidebar: Rename title / Rename subtitle
    Workspace { ws_idx: usize, x: f32, y: f32 },
    /// Terminal surface right-click: Copy surface id (좌표는 logical px 기준)
    TerminalSurface { surface_id: u32, x: f32, y: f32 },
    /// 비-terminal surface (markdown/image/explorer/html 등) right-click (T9).
    /// 전용 항목(현재 copy surface id) + 구분선 + 잘라내기/여기로 이동. 좌표는
    /// logical px 기준. terminal 은 selection-copy 가 있어 `TerminalSurface` 로 분리.
    Surface { surface_id: u32, x: f32, y: f32 },
    /// Explorer surface 내부 우클릭 (T11): 엔트리/다중선택/빈 영역 대상 파일 메뉴.
    /// `paths` 가 비면 빈 영역(=cwd) 대상. `single_is_dir` 는 `paths.len()==1` 일 때만
    /// 유효(폴더 전용 항목 게이팅). 좌표는 logical px.
    Explorer {
        surface_id: u32,
        paths: Vec<std::path::PathBuf>,
        cwd: std::path::PathBuf,
        single_is_dir: bool,
        x: f32,
        y: f32,
    },
    /// Explorer 사이드바 즐겨찾기 항목 우클릭 → "즐겨찾기에서 제거" 전용 메뉴.
    /// 즐겨찾기는 전역이라 surface 식별이 불필요(경로만으로 제거).
    ExplorerFavorite {
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    },
    /// "New workspace" 버튼 우클릭 (full/collapsed sidebar 공통): 프리셋으로 새 워크스페이스 생성
    NewWorkspaceButton { x: f32, y: f32 },
    /// 탭 "+" 버튼 우클릭: 프리셋으로 탭/페인 생성
    NewTabButton { pane_id: u32, x: f32, y: f32 },
}

/// All transient UI dialog/popup state, grouped to avoid AppState bloat.
/// New dialogs should be added here, not as top-level AppState fields.
pub struct DialogState {
    /// Unified rename dialog: target + edit buffer.
    pub(crate) rename: Option<(RenameTarget, String)>,
    /// Convert to markdown: target surface id
    pub(crate) markdown_convert_surface_id: Option<u32>,
    /// Surface convert popup: target surface_id (None = closed)
    pub(crate) convert_popup: Option<u32>,
    /// Keyboard-selected index in the convert popup menu
    pub(crate) convert_popup_selected: Option<usize>,
    /// Pending native context menu
    pub(crate) pending_native_menu: Option<PendingNativeMenu>,
    /// Markdown open popup: path buffer
    pub(crate) markdown_open_buffer: String,
    /// Which pane the file open popup was triggered from
    pub(crate) file_open_pane_id: Option<u32>,
    /// Error message for file open popup validation
    pub(crate) file_open_error: Option<String>,
    /// Pending file drag request (paths to drag to external apps).
    pub(crate) pending_file_drag: Option<Vec<String>>,
    /// Tab drag-and-drop state.
    pub(crate) tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub(crate) ws_drag: Option<WsDragState>,
    /// 부팅 시점 정보/에러 알림용 modal 큐. 큐 head를 [확인] 버튼으로 처리한다.
    /// `crate::adapters::ui::info_modal::show_info_modal()`로 push.
    #[cfg(feature = "gui")]
    pub(crate) info_modal_queue: VecDeque<InfoModal>,
    /// 휴먼 핸드오프 — 응답 대기 중인 approval 큐. popup 의 head 가 현재 화면.
    /// `approval.request` IPC 가 push하고, 선택지 클릭 시 pop.
    pub(crate) pending_approval_ids: VecDeque<tasty_approval::ApprovalId>,
    /// approval popup 의 코멘트 입력 버퍼 (현재 head용 임시 상태).
    pub(crate) approval_comment_buffer: String,
    /// file_handler_picker popup 의 입력/선택 상태. `None` 이면 popup 미오픈.
    pub(crate) file_handler_picker: Option<FileHandlerPickerData>,
    /// 도구 메뉴 클릭 / preset save 후속 — PresetView 를 열어달라는 요청.
    /// `selection` 이 `Some` 이면 PresetView 가 열린 뒤 해당 preset 을 선택한다.
    /// App 메인 루프 `process_pending_open_preset_window` 가 drain.
    pub(crate) pending_open_preset_window: bool,
    /// PresetView 가 열린 뒤 자동 선택할 preset. `pending_open_preset_window` 와 함께 사용.
    pub(crate) pending_preset_window_selection: Option<(tasty_presets::PresetKind, String)>,
    /// 프리셋 적용 picker popup 의 현재 하이라이트 (preset name). popup 닫힘 시 None.
    pub(crate) preset_picker_selected: Option<String>,
    /// `enter_copy_mode` 단축키 트리거 신호. MainView 가 다음 frame 에 소비.
    pub(crate) pending_enter_copy_mode: bool,
}

/// Tab drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct TabDragState {
    pub(crate) pane_id: u32,
    pub(crate) tab_index: usize,
    /// Current mouse x in logical pixels (for insert position calculation).
    pub(crate) current_x: f32,
}

/// 외부 drag&drop hover 중 누적되는 파일 경로 + 시작 cursor 좌표.
/// winit `HoveredFile` 이 N 파일에 대해 N번 발화하므로 `paths` 에 누적.
#[derive(Debug, Clone, Default)]
pub struct DropHoverState {
    pub(crate) paths: Vec<std::path::PathBuf>,
    /// hover 시작 시점의 cursor position (physical pixels). `CursorLeft` 또는
    /// `CursorMoved` 가 drag 중에 발화되지 않을 수 있어 보수적으로 시작점만 기록.
    /// 향후 drop indicator 정밀화 시 read 예정.
    #[allow(dead_code)]
    pub(crate) cursor: Option<(f32, f32)>,
}

/// Workspace drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct WsDragState {
    pub(crate) ws_idx: usize,
    /// Current mouse y in logical pixels (for insert position calculation).
    pub(crate) current_y: f32,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            rename: None,
            markdown_convert_surface_id: None,
            convert_popup: None,
            convert_popup_selected: None,
            pending_native_menu: None,
            markdown_open_buffer: String::new(),
            file_open_pane_id: None,
            file_open_error: None,
            pending_file_drag: None,
            tab_drag: None,
            ws_drag: None,
            #[cfg(feature = "gui")]
            info_modal_queue: VecDeque::new(),
            pending_approval_ids: VecDeque::new(),
            approval_comment_buffer: String::new(),
            file_handler_picker: None,
            pending_preset_window_selection: None,
            pending_open_preset_window: false,
            preset_picker_selected: None,
            pending_enter_copy_mode: false,
        }
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_text_input_open(&self) -> bool {
        self.rename.is_some()
    }

    /// Returns true if any dialog/popup overlay is open.
    pub fn has_any_overlay(&self) -> bool {
        self.has_text_input_open()
    }
}

/// file_handler picker popup 의 한 행 — handler 요약.
#[derive(Debug, Clone)]
pub struct PickerHandlerSummary {
    pub(crate) id: crate::file::handler::HandlerId,
    /// 표시용 라벨. i18n key 가 있으면 번역된 값, 없으면 handler id.
    pub(crate) display: String,
}

/// file_handler picker popup 의 상태.
///
/// 호출자가 popup 을 띄울 때 채워 넣는다. picker 는 직접 dispatch 하지 않고
/// 선택 결과를 [`FileHandlerPickerData::result`] 로 남긴다. host 본체 layer 가
/// frame 끝에 result 를 확인해 실제 핸들러 실행 + RecentPicks 기록을 수행한다.
#[derive(Debug, Clone)]
pub struct FileHandlerPickerData {
    /// 원본 dispatch target. picker 가 닫힌 뒤 host 가 handler 를 실행할 때
    /// 사용한다 — `target_display` 는 화면용이라 escape/축약이 들어갈 수 있다.
    pub(crate) target: crate::file::format::FileTarget,
    /// 표시용 — picker 헤더에 보일 대상 (예: 파일 경로).
    pub(crate) target_display: String,
    /// 탐지된 detector — 없을 수도 있음 ($unknown 등 unmatched).
    pub(crate) detector: Option<crate::file::format::DetectorId>,
    /// 좌측 list 의 후보들 — handler id 사전순.
    pub(crate) candidates: Vec<PickerHandlerSummary>,
    /// 우측 list 의 recent handler ids — 현재 등록된 것만, 저장 파일 순서.
    pub(crate) recent: Vec<PickerHandlerSummary>,
    /// 현재 선택된 handler. 더블클릭/[열기]로 dispatch.
    pub(crate) selected: Option<crate::file::handler::HandlerId>,
    /// dispatch 결과. host 본체 layer 가 frame 끝에서 소비.
    pub(crate) result: Option<FileHandlerPickerResult>,
}

/// picker 의 닫기 사유.
#[derive(Debug, Clone)]
pub enum FileHandlerPickerResult {
    /// 사용자가 handler 선택. host 본체 layer 가 실행 + recent 기록.
    Selected(crate::file::handler::HandlerId),
    /// 취소 또는 ESC — dispatch 없음.
    Cancelled,
}

/// What is being renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// Workspace name.
    WorkspaceName { ws_idx: usize },
    /// Workspace subtitle.
    WorkspaceSubtitle { ws_idx: usize },
    /// Tab name.
    TabName { pane_id: u32, tab_index: usize },
    /// Explorer 파일/폴더 이름 변경 (T11). 대상 surface + 현재 경로.
    ExplorerEntry {
        surface_id: u32,
        path: std::path::PathBuf,
    },
    /// Explorer 즐겨찾기 추가 (T11). rename 팝업과 동일 골격 — buffer = 표시 라벨.
    /// 대상 경로를 그 라벨로 전역 즐겨찾기에 등록한다.
    ExplorerAddFavorite { path: std::path::PathBuf },
}

impl RenameTarget {
    /// i18n key for the dialog heading.
    pub fn heading_key(&self) -> &'static str {
        match self {
            Self::WorkspaceName { .. } => "rename_dialog.title_heading",
            Self::WorkspaceSubtitle { .. } => "rename_dialog.subtitle_heading",
            Self::TabName { .. } => "rename_dialog.tab_heading",
            Self::ExplorerEntry { .. } => "explorer.popup.rename.title",
            Self::ExplorerAddFavorite { .. } => "explorer.popup.add_favorite.title",
        }
    }

    /// Popup scope matching the rename target.
    pub fn popup_scope(&self) -> crate::model::popup_kind::PopupScope {
        match self {
            Self::WorkspaceName { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::WorkspaceSubtitle { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::TabName { pane_id, tab_index } => {
                crate::model::popup_kind::PopupScope::Tab(*pane_id, *tab_index)
            }
            Self::ExplorerEntry { surface_id, .. } => {
                crate::model::popup_kind::PopupScope::Surface(*surface_id)
            }
            // 즐겨찾기는 전역이라 윈도우 스코프(특정 surface 에 묶이지 않음).
            Self::ExplorerAddFavorite { .. } => crate::model::popup_kind::PopupScope::Window,
        }
    }
}

impl AppState {
    /// Memory store 의 lock 안에서 함수를 실행한다. Mutex poisoning 시
    /// poison 해제 후 inner 를 사용 — `Core::with_memory` 와 동일한 정책.
    /// state cleanup / popup draw_fn 등 Core 인자가 cascade 로 도달하지 못하는
    /// 표면에서 동일 port handle 로 접근한다.
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

    /// Creates initial state with one workspace, one pane, one tab, one terminal.
    pub fn new(
        engine: &mut CoreState,
        preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> Self {
        let sidebar_width = engine.settings.appearance.sidebar_width;
        let active_workspace = engine.restored_active_workspace.take().unwrap_or(0);
        Self {
            preset_store,
            memory,
            active_workspace,
            settings_open: false,
            plugins_open: false,
            #[cfg(feature = "gui")]
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            #[cfg(feature = "gui")]
            pending_resize_cursor: None,
            #[cfg(feature = "gui")]
            switch_overlay: None,
            dialogs: DialogState::new(),
            tab_bar_height: PhysicalPx(24.0),
            captured_double_tap: None,
            #[cfg(feature = "gui")]
            pending_surface_keys: Vec::new(),
            pending_lifecycle_events: Vec::new(),
            pending_host_events: Vec::new(),
            last_focused_surface_id: None,
            last_active_workspace_id: None,
            last_focused_tab: None,
            last_tab_locations: None,
            popup_hovered: false,
            banner_hovered: false,
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
            search: crate::search_state::SearchState::new(),
            #[cfg(feature = "gui")]
            port_scan: crate::adapters::ui::popup::port_scanner::PortScanState::Idle,
            command_palette: crate::state::command_palette::CommandPaletteState::default(),
            #[cfg(feature = "gui")]
            toasts: crate::adapters::ui::ToastManager::new(),
            #[cfg(feature = "gui")]
            banners: crate::adapters::ui::BannerManager::new(),
            #[cfg(feature = "gui")]
            markdown_views: Default::default(),
            #[cfg(feature = "gui")]
            image_views: Default::default(),
            #[cfg(feature = "gui")]
            explorer_views: Default::default(),
            tool_registry: crate::plugin::tool_registry::ToolRegistry::new(),
            pending_tool_events: Vec::new(),
            pending_popup_opens: Vec::new(),
            pending_handler_ipc: Vec::new(),
            drop_hover: None,
            pending_file_drops: Vec::new(),
            plugin_popup_events: Vec::new(),
            plugin_popup_closes: Vec::new(),
            plugin_mesh_popup_regions: Vec::new(),
            plugin_mesh_popup_geom: std::collections::HashMap::new(),
            plugin_mesh_popup_bootstrapped: std::collections::HashSet::new(),
            plugin_mesh_popup_theme: std::collections::HashMap::new(),
            pending_intents: Vec::new(),
        }
    }

    /// Intent 발화. `App::dispatch_pending_intents` 가 메인 루프에서 drain.
    /// UI Intent / Domain Intent 모두 본 큐로 발화 (D.3.I.3 두 큐 통합).
    pub fn dispatch_intent(&mut self, intent: crate::intent::DispatchedIntent) {
        self.pending_intents.push(intent);
    }

    /// 현재까지 발화된 Intent 를 모두 꺼내고 큐를 비운다.
    pub fn take_pending_intents(&mut self) -> Vec<crate::intent::DispatchedIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_input_dialog_open(&self) -> bool {
        self.dialogs.has_text_input_open()
    }

    /// Returns true if any egui overlay is visible.
    pub fn has_egui_overlay_open(&self) -> bool {
        let open = self.settings_open || self.plugins_open || self.dialogs.has_any_overlay();
        #[cfg(feature = "gui")]
        let open = open || self.popups.has_any_open();
        open
    }

    /// 닫히는 surface 의 `(surface_id, scrollback_persist_id)` 를 추출. Tab/Pane/Workspace
    /// 닫기 전에 layout 을 한 번 walk 해 결과를 모아두고, 닫기 후에 `cleanup_surface` 에
    /// 전달한다. TerminalSurface 와 deferred EmptySurface 두 케이스를 모두 처리한다.
    pub(crate) fn collect_close_targets(
        tab: &crate::model::Tab,
        engine: &CoreState,
        out: &mut Vec<(u32, Option<String>)>,
    ) {
        tab.for_each_surface(&mut |s| {
            if let Some(ts) = s.as_any().downcast_ref::<crate::model::TerminalSurface>() {
                out.push((
                    ts.id,
                    engine
                        .terminals
                        .scrollback_persist_id(ts.id)
                        .map(str::to_string),
                ));
            } else if let Some(es) = s.as_any().downcast_ref::<crate::model::EmptySurface>() {
                let pid = es
                    .deferred_spawn
                    .as_ref()
                    .and_then(|sp| sp.scrollback_persist_id.clone());
                out.push((es.id, pid));
            }
        });
    }

    /// Clean up all state associated with a closed surface:
    /// surface metadata, per-surface host view state, and memory entries
    /// scoped to this surface (regular + secret).
    ///
    /// `persist_id` 는 닫히는 surface 가 들고 있던 `scrollback_persist_id` 필드값을
    /// 호출자가 미리 뽑아 넘긴다. `Some` 일 때만 `~/.tasty/scrollback/<id>.bin` 파일이
    /// 삭제된다.
    pub(crate) fn cleanup_surface(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
    ) {
        if let Some(pid) = persist_id {
            crate::scrollback_store::delete(&pid);
        }
        engine.pending_scrollback_inject.remove(&surface_id);
        // **D.3.E.4.f** — TerminalStore 의 Terminal/부속 데이터 cascade 정리.
        // store.remove 가 Terminal drop → PTY SIGHUP 발사 + busy/scrollback_persist
        // /deferred/pending_scrollback_inject 까지 함께 정리.
        if let Some(old_terminal) = engine.terminals.remove(surface_id) {
            drop(old_terminal); // SIGHUP — 명시 drop.
        }
        let remove_result =
            self.with_memory(|m| crate::surface_meta::SurfaceMetaStore::remove(m, surface_id));
        if let Err(e) = remove_result {
            tracing::warn!("surface_meta remove failed for surface {surface_id}: {e}");
        }
        #[cfg(feature = "gui")]
        {
            self.markdown_views.drop_view(surface_id);
            self.image_views.drop_view(surface_id);
            self.explorer_views.drop_view(surface_id);
        }
        engine.command_index.drop_surface(surface_id);
        engine.observer_router.drop_surface(surface_id);
        // waker dedup 게이트 제거 — 미제거 시 surface 마다 영구 누적(누수).
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(surface_id);
        }
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

    /// Determine the type of the currently focused surface.
    pub fn focused_surface_type(&self, engine: &CoreState) -> FocusedSurfaceType {
        let pane = match self.focused_pane(engine) {
            Some(p) => p,
            None => return FocusedSurfaceType::None,
        };
        let tab = match pane.tabs.get(pane.active_tab) {
            Some(t) => t,
            None => return FocusedSurfaceType::None,
        };

        // Find the focused leaf surface in the layout
        if let Some(leaf) = tab.layout().find_surface(tab.focused_surface) {
            return Self::surface_to_type(leaf);
        }

        FocusedSurfaceType::None
    }

    fn surface_to_type(surface: &dyn crate::model::Surface) -> FocusedSurfaceType {
        match surface.kind() {
            "terminal" => FocusedSurfaceType::Terminal,
            other => FocusedSurfaceType::Kind(other.to_string()),
        }
    }

    // Explorer 관련 호스트 헬퍼 (focused_explorer_*, explorer_file_*, explorer_select_all
    // 등)는 ExplorerPanel과 함께 제거됨 — 동일 동작을 com.tasty.explorer plugin이
    // 자체 RemoteSurface 안에서 처리한다.

    /// Surface가 close되기 직전에 `kind` 식별자를 얻는다. plugin lifecycle 알림에
    /// payload로 채워 보낸다. None이면 lifecycle 알림을 발행하지 않는다.
    pub fn surface_kind(&self, engine: &CoreState, surface_id: u32) -> Option<&'static str> {
        engine.find_surface_by_id(surface_id).map(|s| s.kind())
    }

    /// Surface close lifecycle 알림 큐에 항목을 추가한다. App 메인 루프가
    /// `take_pending_lifecycle_events`로 drain해서 plugin manager로 dispatch한다.
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

    /// Surface close lifecycle 큐를 비우고 항목을 반환한다.
    pub fn take_pending_lifecycle_events(&mut self) -> Vec<PendingSurfaceClosed> {
        std::mem::take(&mut self.pending_lifecycle_events)
    }

    /// Event Bus 자동 발화 큐에 항목을 추가한다.
    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.pending_host_events.push(event);
    }

    /// Event Bus 자동 발화 큐를 비우고 항목을 반환한다.
    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.pending_host_events)
    }

    /// Cascade 가 host event 를 enqueue 한 직후 polling baseline 을 동기화한다.
    /// 다음 `detect_tab_lifecycle` 호출이 같은 변경을 중복 enqueue 하지 않도록
    /// baseline 을 *현재 engine 상태* 와 일치시키는 역할. baseline 이 아직 `None`
    /// (한 번도 polling 안 함) 이면 no-op — 첫 detect 가 알아서 현재 상태를
    /// 베이스라인으로 잡는다.
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

    /// `lifecycle_baseline_insert_tab` 의 close 대응. 닫힌 tab 을 baseline 에서
    /// 제거해 polling 이 중복 `TabClosed` 발화하지 않도록 한다.
    pub fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32) {
        if let Some(map) = self.last_tab_locations.as_mut() {
            map.remove(&tab_id);
        }
    }

    /// Get the working directory to inherit from the focused surface, if enabled.
    ///
    /// 사용자가 현재 포커스한 surface 본인의 `source_cwd()`를 사용한다.
    /// (terminal/explorer/markdown/html → 자체 cwd, image/empty/clipboard → None)
    pub(crate) fn resolve_inherit_cwd(&self, engine: &CoreState) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd || engine.workspaces.is_empty() {
            return None;
        }
        let sid = self.focused_surface_id(engine)?;
        cwd_from_surface(engine, sid)
    }

    /// Get the working directory to inherit from a specific surface, if enabled.
    pub(crate) fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd {
            return None;
        }
        cwd_from_surface(engine, surface_id)
    }
}

/// surface_id 의 source_cwd 를 결정. Terminal kind 는 store 의 Terminal.get_cwd(),
/// 그 외 kind 는 Surface trait 의 default source_cwd() (markdown 은 파일 부모 등).
fn cwd_from_surface(engine: &CoreState, surface_id: u32) -> Option<std::path::PathBuf> {
    let surface = engine.find_surface_by_id(surface_id)?;
    if surface.kind() == "terminal" {
        engine.terminals.get(surface_id).and_then(|t| t.get_cwd())
    } else {
        surface.source_cwd()
    }
}
