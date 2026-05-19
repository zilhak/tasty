mod focus;
mod layout;
mod mark;
mod message;
mod mouse;
mod pane;
pub mod preset_apply;
mod restore;
mod tab;
#[cfg(test)]
mod tests;
mod workspace;

use std::collections::VecDeque;

use crate::engine_state::EngineState;
use crate::model::{LogicalPx, PhysicalPx};
use crate::settings_ui::SettingsUiState;
use crate::ui::info_modal::InfoModal;
use tasty_terminal::{Terminal, TerminalEvent, Waker};

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
#[derive(Debug, Clone)]
pub struct PendingKeyEvent {
    pub key: winit::keyboard::Key,
    pub modifiers: winit::keyboard::ModifiersState,
    pub text: Option<winit::keyboard::SmolStr>,
}

#[derive(Debug, Clone)]
pub struct SurfaceMessage {
    pub id: u32,
    pub from_surface_id: u32,
    pub content: String,
}

/// Surface가 닫혔다는 사실을 plugin 측에 broadcast하기 위해 메인 루프가 소비할
/// 큐 항목. `state/`는 `plugin/` 의존이 없으므로 enum 대신 `is_user_close: bool`로
/// reason을 담고, App 메인 루프에서 `SurfaceCloseReason`으로 매핑한다.
#[derive(Debug, Clone)]
pub struct PendingSurfaceClosed {
    pub surface_id: u32,
    pub kind: &'static str,
    pub is_user_close: bool,
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
    SurfaceResized {
        surface_id: u32,
        width_px: u32,
        height_px: u32,
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
}

// IdGenerator is now in engine_state.rs

pub struct AppState {
    /// Engine-level shared state (workspaces, terminals, settings, hooks, etc.)
    pub engine: EngineState,

    // ── Window-level UI state ──
    pub active_workspace: usize,
    /// Whether the settings window is open.
    pub settings_open: bool,
    /// Whether the plugins window is open.
    pub plugins_open: bool,
    /// Persistent UI state for the settings window.
    pub settings_ui_state: SettingsUiState,
    /// Cached sidebar width from settings (logical pixels).
    pub sidebar_width: LogicalPx,
    /// Sidebar visibility: false = completely hidden.
    pub sidebar_visible: bool,
    /// Sidebar collapsed: true = compact mode (narrow width, icons only).
    pub sidebar_collapsed: bool,
    /// All transient dialog/popup state.
    pub dialogs: DialogState,
    /// Measured tab bar height in physical pixels, updated each frame by egui.
    pub tab_bar_height: PhysicalPx,
    /// Popup manager for internal popups (notification panel, etc.).
    pub popups: crate::ui::PopupManager,
    /// Terminal text search state.
    pub search: crate::search_state::SearchState,
    /// TCP listening port scan cache, keyed by surface_id.
    /// Refreshed lazily when the ports popup is opened or visible.
    pub port_scan: tasty_portscan::PortScanCache,
    /// Shared snapshot of background update-check state. Polled hourly.
    pub update_status: std::sync::Arc<std::sync::Mutex<crate::update_check::UpdateStatus>>,
    /// Command palette UI state — query buffer, selection cursor, and a pending
    /// dispatch slot that MainWindow drains each frame.
    pub command_palette: crate::command_palette::CommandPaletteState,
    /// Toast manager for transient in-app notifications (copy feedback, etc.).
    /// 사용자 행동에서만 발사한다. CLI/IPC 경유 동작은 토스트를 만들지 않는다.
    pub toasts: crate::ui::ToastManager,
    /// Cached recent files list (markdown/html open popups). Loaded from disk at
    /// startup and mutated in-place; each mutation saves back to disk.
    pub recent_files: crate::recent_files::RecentFiles,
    /// Whether the mouse is currently over an open popup (input layer state).
    /// Updated each frame by PopupManager::draw(). Mouse handlers check this
    /// to block events from reaching lower layers (terminal, dividers).
    pub popup_hovered: bool,
    /// Double-tap modifier captured from winit events, for the keybinding recorder to consume.
    pub captured_double_tap: Option<String>,
    /// Keyboard events for non-terminal surfaces, consumed during egui rendering.
    pub pending_surface_keys: Vec<PendingKeyEvent>,
    /// Surface close lifecycle 알림 큐. close 직후 enqueue되고, App 메인 루프가
    /// drain하여 `PluginManager::notify_surface_closed`로 dispatch한다.
    /// `state/`는 `plugin/` 의존이 없어 별도 plain struct로 둔다.
    pub pending_lifecycle_events: Vec<PendingSurfaceClosed>,
    /// Event Bus 1.0 호스트 자동 발화 큐. 호스트 코드 곳곳에서 `enqueue_host_event`로
    /// push하고, App 메인 루프가 drain해 wire payload로 변환·발화한다.
    pub pending_host_events: Vec<PendingHostEvent>,
    /// `surface.focused` 발화용 변화 감지 상태. tick마다 `focused_surface_id()`와
    /// 비교해 달라졌으면 `SurfaceFocused`를 enqueue한다. focus 전환 경로가 많아
    /// (키보드/마우스/IPC/탭전환/워크스페이스전환) 각각을 hook하기보다 polling이 단순.
    pub last_focused_surface_id: Option<u32>,
    /// `workspace.activated` 발화용 변화 감지 상태. `active_workspace` 인덱스가
    /// 가리키는 워크스페이스 ID를 기록해 두고, 다음 tick에서 달라졌다면
    /// `WorkspaceActivated`를 enqueue한다.
    pub last_active_workspace_id: Option<u32>,
    /// `tab.focused` 발화용 변화 감지 상태. 활성 워크스페이스의 focused pane이 보유한
    /// 현재 active tab의 (pane_id, tab_id)를 기록. 다음 tick에서 달라졌다면
    /// `TabFocused`를 enqueue한다. pane 전환·in-pane tab 전환을 한꺼번에 다룬다.
    pub last_focused_tab: Option<(u32, u32)>,
    /// `tab.created`/`tab.closed`/`tab.moved` 발화용 polling 상태. tab_id →
    /// (pane_id, workspace_id, kind) 스냅샷. `None`은 아직 한 번도 polling하지
    /// 않은 상태(초기 로드된 탭에 대해 spurious `tab.created`가 발화되는 것을 막기
    /// 위해 첫 호출에서는 스냅샷만 만들고 이벤트를 enqueue하지 않는다).
    pub last_tab_locations: Option<std::collections::HashMap<u32, (u32, u32, String)>>,
    /// `pane.created`/`pane.closed` polling 상태. pane_id → workspace_id 스냅샷.
    /// `last_tab_locations`와 동일한 초기 베이스라인 정책 적용.
    pub last_pane_locations: Option<std::collections::HashMap<u32, u32>>,
    /// `workspace.created`/`workspace.closed` polling 상태. workspace_id → name
    /// 스냅샷. 첫 호출에서는 베이스라인만 기록.
    pub last_workspace_snapshot: Option<std::collections::HashMap<u32, String>>,
    /// `surface.created` polling 상태. surface_id → (tab_id, pane_id, ws_id, kind)
    /// 스냅샷. 첫 호출은 베이스라인만 기록. `surface.closed`는 별도 큐로
    /// 이미 발화하므로 여기서는 신규 생성만 감지한다.
    pub last_surface_locations:
        Option<std::collections::HashMap<u32, (u32, u32, u32, &'static str)>>,

    /// Per-surface host view state for `MarkdownPanel` (content cache, scroll, commonmark cache).
    /// `MarkdownPanel` itself only holds `file_path` + reload tracking; everything GUI-bound lives here.
    pub markdown_views: crate::ui::markdown_view::MarkdownViewStore,

    /// Per-surface host view state for `ImagePanel` (pixel buffer, textures, edit state,
    /// undo history, brush settings, popup buffers).
    pub image_views: crate::ui::image_view::ImageViewStore,

    /// 사이드바 도구 메뉴 항목. 활성 plugin의 `[[contributes.tool]]`
    /// 항목을 합쳐 관리. PluginManager가 plugin 라이프사이클 변경 시
    /// `set_plugin_items(mgr.plugin_tool_items())`로 갱신한다.
    pub tool_registry: crate::plugin::tool_registry::ToolRegistry,

    /// 도구 메뉴 항목 클릭 시 publish해야 할 이벤트 큐. tools_menu가 `&mut AppState`만
    /// 가지므로 PluginManager에 직접 접근할 수 없어, 클릭 시점에 enqueue하고 App 메인
    /// 루프가 drain해 `PluginManager::emit_host_event`로 발화한다.
    pub pending_tool_events: Vec<(String, serde_json::Value)>,

    /// `ToolAction::OpenPopup` 클릭 시 열어야 할 popup 큐.
    /// (plugin_id, popup_id, context). App 메인 루프가 drain해
    /// `PluginManager::open_popup_instance`로 dispatch.
    pub pending_popup_opens: Vec<(String, String, serde_json::Value)>,

    /// file_handler 디스패치 결과가 plugin IPC method 일 때의 호출 큐.
    /// `(ipc_method, target)`. App 메인 루프가 drain 해 `PluginManager` 로 forward.
    pub pending_handler_ipc: Vec<(String, crate::file_format::FileTarget)>,

    /// 외부 drag&drop 으로 파일이 hover 중인 상태. `HoveredFile` 마다 path 누적,
    /// `HoveredFileCancelled` / `DroppedFile` 시 해제. 비주얼 overlay 의 입력.
    pub drop_hover: Option<DropHoverState>,

    /// `DroppedFile` 이벤트로 받은 경로 큐. frame end 에서 drain 해
    /// `file_dispatch::dispatch_file_target` 으로 보낸다.
    pub pending_file_drops: Vec<std::path::PathBuf>,

    /// plugin popup 렌더 중 수집된 사용자 입력. App 메인 루프가 drain해
    /// `PluginManager::send_popup_event`로 forward한다.
    pub plugin_popup_events: Vec<(u64, tasty_plugin_protocol::ui_tree::UiEvent)>,

    /// plugin popup 렌더 중 감지된 close 사유 (outside-click / Escape).
    /// App 메인 루프가 drain해 `PluginManager::close_popup_instance`를 호출한다.
    pub plugin_popup_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>,
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
    Workspace {
        ws_idx: usize,
        x: f32,
        y: f32,
    },
    /// Terminal surface right-click: Copy surface id (좌표는 logical px 기준)
    TerminalSurface {
        surface_id: u32,
        x: f32,
        y: f32,
    },
}

/// All transient UI dialog/popup state, grouped to avoid AppState bloat.
/// New dialogs should be added here, not as top-level AppState fields.
pub struct DialogState {
    /// Unified rename dialog: target + edit buffer.
    pub rename: Option<(RenameTarget, String)>,
    /// Convert to markdown: target surface id
    pub markdown_convert_surface_id: Option<u32>,
    /// Surface convert popup: target surface_id (None = closed)
    pub convert_popup: Option<u32>,
    /// Keyboard-selected index in the convert popup menu
    pub convert_popup_selected: Option<usize>,
    /// Convert to html: target surface id
    pub html_convert_surface_id: Option<u32>,
    /// Pending native context menu
    pub pending_native_menu: Option<PendingNativeMenu>,
    /// Markdown open popup: path buffer
    pub markdown_open_buffer: String,
    /// HTML open popup: url buffer
    pub html_open_buffer: String,
    /// Which pane the file open popup was triggered from
    pub file_open_pane_id: Option<u32>,
    /// Internal flag for cancel button in file open popups
    pub file_popup_cancel: bool,
    /// Error message for file open popup validation
    pub file_open_error: Option<String>,
    /// Deferred popup open request: (popup_id, scope). Processed after popup draw loop.
    pub pending_popup_open: Option<(&'static str, crate::ui::popup::PopupScope)>,
    /// Pending file drag request (paths to drag to external apps).
    pub pending_file_drag: Option<Vec<String>>,
    /// Tab drag-and-drop state.
    pub tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub ws_drag: Option<WsDragState>,
    /// 부팅 시점 정보/에러 알림용 modal 큐. 큐 head를 [확인] 버튼으로 처리한다.
    /// `crate::ui::info_modal::show_info_modal()`로 push.
    pub info_modal_queue: VecDeque<InfoModal>,
    /// 휴먼 핸드오프 — 응답 대기 중인 approval 큐. popup 의 head 가 현재 화면.
    /// `approval.request` IPC 가 push하고, 선택지 클릭 시 pop.
    pub pending_approval_ids: VecDeque<tasty_approval::ApprovalId>,
    /// approval popup 의 코멘트 입력 버퍼 (현재 head용 임시 상태).
    pub approval_comment_buffer: String,
    /// file_handler_picker popup 의 입력/선택 상태. `None` 이면 popup 미오픈.
    pub file_handler_picker: Option<FileHandlerPickerData>,
    /// Git viewer popup 의 현재 상태. popup 닫힘 시 `None` 으로 리셋.
    pub git_viewer: Option<crate::git_viewer::GitViewerState>,
}

/// Tab drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct TabDragState {
    pub pane_id: u32,
    pub tab_index: usize,
    /// Current mouse x in logical pixels (for insert position calculation).
    pub current_x: f32,
}

/// 외부 drag&drop hover 중 누적되는 파일 경로 + 시작 cursor 좌표.
/// winit `HoveredFile` 이 N 파일에 대해 N번 발화하므로 `paths` 에 누적.
#[derive(Debug, Clone, Default)]
pub struct DropHoverState {
    pub paths: Vec<std::path::PathBuf>,
    /// hover 시작 시점의 cursor position (physical pixels). `CursorLeft` 또는
    /// `CursorMoved` 가 drag 중에 발화되지 않을 수 있어 보수적으로 시작점만 기록.
    pub cursor: Option<(f32, f32)>,
}

/// Workspace drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct WsDragState {
    pub ws_idx: usize,
    /// Current mouse y in logical pixels (for insert position calculation).
    pub current_y: f32,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            rename: None,
            markdown_convert_surface_id: None,
            convert_popup: None,
            convert_popup_selected: None,
            html_convert_surface_id: None,
            pending_native_menu: None,
            markdown_open_buffer: String::new(),
            html_open_buffer: String::new(),
            file_open_pane_id: None,
            file_popup_cancel: false,
            file_open_error: None,
            pending_popup_open: None,
            pending_file_drag: None,
            tab_drag: None,
            ws_drag: None,
            info_modal_queue: VecDeque::new(),
            pending_approval_ids: VecDeque::new(),
            approval_comment_buffer: String::new(),
            file_handler_picker: None,
            git_viewer: None,
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
    pub id: crate::file_handler::HandlerId,
    /// 표시용 라벨. i18n key 가 있으면 번역된 값, 없으면 handler id.
    pub display: String,
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
    pub target: crate::file_format::FileTarget,
    /// 표시용 — picker 헤더에 보일 대상 (예: 파일 경로).
    pub target_display: String,
    /// 탐지된 detector — 없을 수도 있음 ($unknown 등 unmatched).
    pub detector: Option<crate::file_format::DetectorId>,
    /// 좌측 list 의 후보들 — handler id 사전순.
    pub candidates: Vec<PickerHandlerSummary>,
    /// 우측 list 의 recent handler ids — 현재 등록된 것만, 저장 파일 순서.
    pub recent: Vec<PickerHandlerSummary>,
    /// 현재 선택된 handler. 더블클릭/[열기]로 dispatch.
    pub selected: Option<crate::file_handler::HandlerId>,
    /// dispatch 결과. host 본체 layer 가 frame 끝에서 소비.
    pub result: Option<FileHandlerPickerResult>,
}

/// picker 의 닫기 사유.
#[derive(Debug, Clone)]
pub enum FileHandlerPickerResult {
    /// 사용자가 handler 선택. host 본체 layer 가 실행 + recent 기록.
    Selected(crate::file_handler::HandlerId),
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
}

impl RenameTarget {
    /// i18n key for the dialog heading.
    pub fn heading_key(&self) -> &'static str {
        match self {
            Self::WorkspaceName { .. } => "rename_dialog.title_heading",
            Self::WorkspaceSubtitle { .. } => "rename_dialog.subtitle_heading",
            Self::TabName { .. } => "rename_dialog.tab_heading",
        }
    }

    /// Popup scope matching the rename target.
    pub fn popup_scope(&self) -> crate::ui::popup::PopupScope {
        match self {
            Self::WorkspaceName { ws_idx } => crate::ui::popup::PopupScope::Workspace(*ws_idx),
            Self::WorkspaceSubtitle { ws_idx } => crate::ui::popup::PopupScope::Workspace(*ws_idx),
            Self::TabName { pane_id, tab_index } => {
                crate::ui::popup::PopupScope::Tab(*pane_id, *tab_index)
            }
        }
    }
}

impl AppState {
    /// Creates initial state with one workspace, one pane, one tab, one terminal.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let mut engine = EngineState::new(cols, rows, waker)?;
        let sidebar_width = engine.settings.appearance.sidebar_width;
        let active_workspace = engine.restored_active_workspace.take().unwrap_or(0);
        Ok(Self {
            engine,
            active_workspace,
            settings_open: false,
            plugins_open: false,
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            dialogs: DialogState::new(),
            tab_bar_height: PhysicalPx(24.0),
            captured_double_tap: None,
            pending_surface_keys: Vec::new(),
            pending_lifecycle_events: Vec::new(),
            pending_host_events: Vec::new(),
            last_focused_surface_id: None,
            last_active_workspace_id: None,
            last_focused_tab: None,
            last_tab_locations: None,
            last_pane_locations: None,
            last_workspace_snapshot: None,
            last_surface_locations: None,
            popup_hovered: false,
            recent_files: crate::recent_files::RecentFiles::load(),
            popups: {
                let mut pm = crate::ui::PopupManager::new();
                for def in crate::ui::popup_defs::all_defs() {
                    pm.register_def(def);
                }
                pm
            },
            search: crate::search_state::SearchState::new(),
            port_scan: tasty_portscan::PortScanCache::new(tasty_portscan::DEFAULT_TTL),
            update_status: crate::update_check::spawn_poller(
                "zilhak",
                "tasty",
                env!("CARGO_PKG_VERSION"),
                std::time::Duration::from_secs(60 * 60),
            ),
            command_palette: crate::command_palette::CommandPaletteState::default(),
            toasts: crate::ui::ToastManager::new(),
            markdown_views: Default::default(),
            image_views: Default::default(),
            tool_registry: crate::plugin::tool_registry::ToolRegistry::new(),
            pending_tool_events: Vec::new(),
            pending_popup_opens: Vec::new(),
            pending_handler_ipc: Vec::new(),
            drop_hover: None,
            pending_file_drops: Vec::new(),
            plugin_popup_events: Vec::new(),
            plugin_popup_closes: Vec::new(),
        })
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_input_dialog_open(&self) -> bool {
        self.dialogs.has_text_input_open()
    }

    /// Returns true if any egui overlay is visible.
    pub fn has_egui_overlay_open(&self) -> bool {
        self.settings_open
            || self.plugins_open
            || self.dialogs.has_any_overlay()
            || self.popups.has_any_open()
    }

    /// 닫히는 surface 의 `(surface_id, scrollback_persist_id)` 를 추출. Tab/Pane/Workspace
    /// 닫기 전에 layout 을 한 번 walk 해 결과를 모아두고, 닫기 후에 `cleanup_surface` 에
    /// 전달한다. TerminalSurface 와 deferred EmptySurface 두 케이스를 모두 처리한다.
    pub(crate) fn collect_close_targets(
        tab: &crate::model::Tab,
        out: &mut Vec<(u32, Option<String>)>,
    ) {
        tab.for_each_surface(&mut |s| {
            if let Some(ts) = s.as_terminal_surface() {
                out.push((ts.id, ts.scrollback_persist_id.clone()));
            } else if let Some(es) =
                s.as_any().downcast_ref::<crate::model::EmptySurface>()
            {
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
    pub(crate) fn cleanup_surface(&mut self, surface_id: u32, persist_id: Option<String>) {
        if let Some(pid) = persist_id {
            crate::scrollback_store::delete(&pid);
        }
        self.engine.pending_scrollback_inject.remove(&surface_id);
        if let Err(e) = crate::surface_meta::SurfaceMetaStore::remove(surface_id) {
            tracing::warn!("surface_meta remove failed for surface {surface_id}: {e}");
        }
        self.markdown_views.drop_view(surface_id);
        self.image_views.drop_view(surface_id);
        self.engine.command_index.drop_surface(surface_id);
        self.engine.observer_router.drop_surface(surface_id);
        let scope = tasty_memory::Scope::Surface(surface_id);
        tasty_memory::with_store(|s| match s.purge_scope(&scope) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                surface_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-surface scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(surface_id, "memory: purge_scope failed: {e}"),
        });
    }

    /// Invariant: caller must ensure `engine.workspaces` is non-empty.
    /// Parked states (after the last window closes) can have zero workspaces —
    /// such callers must use `engine.workspaces.is_empty()` checks instead.
    pub fn active_workspace(&self) -> &crate::model::Workspace {
        debug_assert!(
            !self.engine.workspaces.is_empty(),
            "active_workspace called with empty workspaces"
        );
        let idx = self
            .active_workspace
            .min(self.engine.workspaces.len().saturating_sub(1));
        &self.engine.workspaces[idx]
    }

    pub fn active_workspace_mut(&mut self) -> &mut crate::model::Workspace {
        debug_assert!(
            !self.engine.workspaces.is_empty(),
            "active_workspace_mut called with empty workspaces"
        );
        let idx = self
            .active_workspace
            .min(self.engine.workspaces.len().saturating_sub(1));
        &mut self.engine.workspaces[idx]
    }

    /// Get the focused pane in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    pub fn focused_pane(&self) -> Option<&crate::model::Pane> {
        if self.engine.workspaces.is_empty() {
            return None;
        }
        let ws = self.active_workspace();
        let layout = ws.pane_layout();
        layout
            .find_pane(ws.focused_pane)
            .or_else(|| layout.first_pane())
    }

    /// Get the focused pane (mutable) in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    pub fn focused_pane_mut(&mut self) -> Option<&mut crate::model::Pane> {
        if self.engine.workspaces.is_empty() {
            return None;
        }
        let ws = self.active_workspace_mut();
        let focused_id = ws.focused_pane;
        // If focused_id is stale, fall back to the first available pane.
        if ws.pane_layout().find_pane(focused_id).is_none() {
            let fallback_id = ws.pane_layout().first_pane().map(|p| p.id);
            if let Some(fid) = fallback_id {
                ws.focused_pane = fid;
            }
        }
        let focused_id = ws.focused_pane;
        ws.pane_layout_mut().find_pane_mut(focused_id)
    }

    /// Get the focused surface ID (the surface that currently receives input).
    pub fn focused_surface_id(&self) -> Option<u32> {
        let pane = self.focused_pane()?;
        let tab = pane.tabs.get(pane.active_tab)?;
        tab.focused_surface_id()
    }

    /// Recompute `engine.busy_surfaces` by polling every PTY's foreground
    /// process. Returns true if the set changed (caller should redraw).
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        let mut busy: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for ws in &mut self.engine.workspaces {
            ws.pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, terminal| {
                    if terminal.is_busy() {
                        busy.insert(sid);
                    }
                });
        }
        let changed = self.engine.busy_surfaces != busy;
        self.engine.busy_surfaces = busy;
        changed
    }

    /// Whether the given surface is currently running a non-shell foreground
    /// program (cached value from the last `refresh_busy_surfaces` poll).
    pub fn is_surface_busy(&self, surface_id: u32) -> bool {
        self.engine.busy_surfaces.contains(&surface_id)
    }

    /// Whether any surface in the given list is busy.
    pub fn any_busy(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|sid| self.engine.busy_surfaces.contains(sid))
    }

    /// Number of busy surfaces among the given list.
    pub fn busy_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.engine.busy_surfaces.contains(sid))
            .count()
    }

    /// Determine the type of the currently focused surface.
    pub fn focused_surface_type(&self) -> FocusedSurfaceType {
        let pane = match self.focused_pane() {
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

    /// Record that the user typed on the given surface (updates last_key_input timestamp).
    pub fn record_typing(&mut self, surface_id: u32) {
        self.engine
            .last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    /// Returns true if the surface received key input within the last 5 seconds.
    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.engine.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    pub(crate) fn send_fast_init(&mut self, surface_id: u32) {
        self.engine.send_fast_init(surface_id);
    }

    /// Find a surface (any type) by ID across all workspaces.
    pub fn find_surface_by_id(&self, surface_id: u32) -> Option<&dyn crate::model::Surface> {
        for workspace in &self.engine.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if !tab.contains_surface(surface_id) {
                            continue;
                        }
                        if let Some(s) = tab.layout().find_surface(surface_id) {
                            return Some(s);
                        }
                    }
                }
            }
        }
        None
    }

    /// Surface가 close되기 직전에 `kind` 식별자를 얻는다. plugin lifecycle 알림에
    /// payload로 채워 보낸다. None이면 lifecycle 알림을 발행하지 않는다.
    pub fn surface_kind(&self, surface_id: u32) -> Option<&'static str> {
        self.find_surface_by_id(surface_id).map(|s| s.kind())
    }

    /// Surface close lifecycle 알림 큐에 항목을 추가한다. App 메인 루프가
    /// `take_pending_lifecycle_events`로 drain해서 plugin manager로 dispatch한다.
    pub fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: &'static str,
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

    /// 현재 focused surface id를 마지막 기록과 비교해 달라졌다면 `SurfaceFocused`
    /// 이벤트를 enqueue하고 기록을 갱신한다. focus 전환 경로(키/마우스/IPC/탭/워크
    /// 스페이스)가 많아 각각 hook하는 대신 main loop tick에서 polling으로 처리한다.
    pub fn detect_focus_change(&mut self) {
        let current = self.focused_surface_id();
        if current == self.last_focused_surface_id {
            return;
        }
        let prev = self.last_focused_surface_id;
        self.last_focused_surface_id = current;
        if let Some(surface_id) = current {
            self.enqueue_host_event(PendingHostEvent::SurfaceFocused {
                surface_id,
                prev_surface_id: prev,
            });
        }
    }

    /// 현재 활성 워크스페이스 ID를 마지막 기록과 비교해 달라졌다면 `WorkspaceActivated`
    /// 이벤트를 enqueue한다. workspace 활성화 경로(사이드바 클릭, 단축키, IPC 등)가
    /// 여럿이라 focused와 동일하게 polling으로 처리.
    pub fn detect_workspace_activation(&mut self) {
        let current = self
            .engine
            .workspaces
            .get(self.active_workspace)
            .map(|w| w.id);
        if current == self.last_active_workspace_id {
            return;
        }
        let prev = self.last_active_workspace_id;
        self.last_active_workspace_id = current;
        if let Some(workspace_id) = current {
            self.enqueue_host_event(PendingHostEvent::WorkspaceActivated {
                workspace_id,
                prev_workspace_id: prev,
            });
        }
    }

    /// focused pane의 active tab을 마지막 기록과 비교해 달라졌다면 `TabFocused`
    /// 이벤트를 enqueue. tab 전환 경로(클릭, next/prev/goto 단축키, close 후 인접
    /// 탭으로 shift, pane 전환에 의한 focused tab 변화 등)가 여럿이라 polling 채택.
    pub fn detect_tab_focus_change(&mut self) {
        let current = self.focused_pane().and_then(|pane| {
            pane.tabs.get(pane.active_tab).map(|tab| (pane.id, tab.id))
        });
        if current == self.last_focused_tab {
            return;
        }
        let prev_tab_id = self.last_focused_tab.map(|(_, tab_id)| tab_id);
        self.last_focused_tab = current;
        if let Some((pane_id, tab_id)) = current {
            self.enqueue_host_event(PendingHostEvent::TabFocused {
                tab_id,
                pane_id,
                prev_tab_id,
            });
        }
    }

    /// 전체 워크스페이스를 순회하며 현재 (tab_id → pane_id, workspace_id, kind) 매핑을
    /// 마지막 스냅샷과 비교해 `TabCreated`/`TabClosed`/`TabMoved` 이벤트를 enqueue한다.
    /// 첫 호출(스냅샷이 `None`)에서는 이벤트를 발화하지 않고 베이스라인만 기록한다 —
    /// 앱 시작 시 이미 로드된 탭들이 잘못 `tab.created`로 보고되지 않도록 하기 위함.
    pub fn detect_tab_lifecycle(&mut self) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, (u32, u32, String)> = HashMap::new();
        for ws in &self.engine.workspaces {
            let workspace_id = ws.id;
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        let kind = tab
                            .focused_surface_id()
                            .and_then(|sid| self.find_surface_by_id(sid))
                            .map(|s| s.kind().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        current.insert(tab.id, (pane_id, workspace_id, kind));
                    }
                }
            }
        }

        let prev = match self.last_tab_locations.take() {
            Some(p) => p,
            None => {
                self.last_tab_locations = Some(current);
                return;
            }
        };

        for (tab_id, (pane_id, workspace_id, kind)) in &current {
            match prev.get(tab_id) {
                None => {
                    self.pending_host_events.push(PendingHostEvent::TabCreated {
                        tab_id: *tab_id,
                        pane_id: *pane_id,
                        workspace_id: *workspace_id,
                        kind: kind.clone(),
                    });
                }
                Some((prev_pane, _, _)) if prev_pane != pane_id => {
                    self.pending_host_events.push(PendingHostEvent::TabMoved {
                        tab_id: *tab_id,
                        from_pane: *prev_pane,
                        to_pane: *pane_id,
                    });
                }
                _ => {}
            }
        }
        for (tab_id, (pane_id, _, _)) in &prev {
            if !current.contains_key(tab_id) {
                self.pending_host_events.push(PendingHostEvent::TabClosed {
                    tab_id: *tab_id,
                    pane_id: *pane_id,
                });
            }
        }

        self.last_tab_locations = Some(current);
    }

    /// Pane 생성/종료를 polling으로 감지. `last_tab_locations`와 동일하게 첫 호출
    /// 에서는 베이스라인만 기록한다.
    pub fn detect_pane_lifecycle(&mut self) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, u32> = HashMap::new();
        for ws in &self.engine.workspaces {
            for pane_id in ws.pane_layout().all_pane_ids() {
                current.insert(pane_id, ws.id);
            }
        }

        let prev = match self.last_pane_locations.take() {
            Some(p) => p,
            None => {
                self.last_pane_locations = Some(current);
                return;
            }
        };

        for (pane_id, workspace_id) in &current {
            if !prev.contains_key(pane_id) {
                self.pending_host_events.push(PendingHostEvent::PaneCreated {
                    pane_id: *pane_id,
                    workspace_id: *workspace_id,
                });
            }
        }
        for pane_id in prev.keys() {
            if !current.contains_key(pane_id) {
                self.pending_host_events.push(PendingHostEvent::PaneClosed {
                    pane_id: *pane_id,
                });
            }
        }

        self.last_pane_locations = Some(current);
    }

    /// Workspace 생성/종료를 polling으로 감지. `window_id`는 caller가 전달하며
    /// (이 `AppState`가 속한 main window의 winit::WindowId를 u64로 변환), 신규
    /// workspace가 발견되면 `WorkspaceCreated`에 채워 넣는다.
    pub fn detect_workspace_lifecycle(&mut self, window_id: u64) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, String> = HashMap::new();
        for ws in &self.engine.workspaces {
            current.insert(ws.id, ws.name.clone());
        }

        let prev = match self.last_workspace_snapshot.take() {
            Some(p) => p,
            None => {
                self.last_workspace_snapshot = Some(current);
                return;
            }
        };

        for (workspace_id, name) in &current {
            if !prev.contains_key(workspace_id) {
                self.pending_host_events
                    .push(PendingHostEvent::WorkspaceCreated {
                        workspace_id: *workspace_id,
                        window_id,
                        name: name.clone(),
                    });
            }
        }
        for workspace_id in prev.keys() {
            if !current.contains_key(workspace_id) {
                self.pending_host_events
                    .push(PendingHostEvent::WorkspaceClosed {
                        workspace_id: *workspace_id,
                    });
            }
        }

        self.last_workspace_snapshot = Some(current);
    }

    /// Surface 생성을 polling으로 감지. 신규 surface_id가 발견되면 `SurfaceCreated`를
    /// `created_by_plugin: None` (User)로 enqueue한다. Plugin이 spawn한 surface는
    /// 향후 plugin spawn IPC 핸들러에서 별도로 `Agent { source_plugin }` 컨텍스트를
    /// 채워 직접 enqueue하는 경로를 둘 예정. surface.closed는 별도 큐가 처리하므로
    /// 여기서는 생성만 감지.
    pub fn detect_surface_lifecycle(&mut self) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, (u32, u32, u32, &'static str)> = HashMap::new();
        for ws in &self.engine.workspaces {
            let workspace_id = ws.id;
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        let tab_id = tab.id;
                        if let Some(layout) = tab.layout_if_initialized() {
                            for sid in layout.all_surface_ids() {
                                let kind = layout
                                    .find_surface(sid)
                                    .map(|s| s.kind())
                                    .unwrap_or("unknown");
                                current.insert(sid, (tab_id, pane_id, workspace_id, kind));
                            }
                        }
                    }
                }
            }
        }

        let prev = match self.last_surface_locations.take() {
            Some(p) => p,
            None => {
                self.last_surface_locations = Some(current);
                return;
            }
        };

        for (surface_id, (tab_id, pane_id, workspace_id, kind)) in &current {
            if !prev.contains_key(surface_id) {
                self.pending_host_events.push(PendingHostEvent::SurfaceCreated {
                    surface_id: *surface_id,
                    kind,
                    tab_id: *tab_id,
                    pane_id: *pane_id,
                    workspace_id: *workspace_id,
                    created_by_plugin: None,
                });
            }
        }

        self.last_surface_locations = Some(current);
    }

    /// Get the working directory to inherit from the focused surface, if enabled.
    ///
    /// 사용자가 현재 포커스한 surface 본인의 `source_cwd()`를 사용한다.
    /// (terminal/explorer/markdown/html → 자체 cwd, image/empty/clipboard → None)
    pub(crate) fn resolve_inherit_cwd(&self) -> Option<std::path::PathBuf> {
        if !self.engine.settings.general.inherit_cwd || self.engine.workspaces.is_empty() {
            return None;
        }
        let sid = self.focused_surface_id()?;
        self.find_surface_by_id(sid)
            .and_then(|s| s.source_cwd())
    }

    /// Get the working directory to inherit from a specific surface, if enabled.
    pub(crate) fn resolve_inherit_cwd_from_surface(
        &self,
        surface_id: u32,
    ) -> Option<std::path::PathBuf> {
        if !self.engine.settings.general.inherit_cwd {
            return None;
        }
        self.find_surface_by_id(surface_id)
            .and_then(|s| s.source_cwd())
    }

    /// Get the ultimately focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        self.focused_pane().and_then(|p| p.active_terminal())
    }

    /// Get the ultimately focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.focused_pane_mut()
            .and_then(|p| p.active_terminal_mut())
    }

    /// Get the focused image panel (mutable).
    pub fn focused_image_mut(&mut self) -> Option<&mut crate::model::ImagePanel> {
        let pane = self.focused_pane_mut()?;
        let tab = pane.tabs.get_mut(pane.active_tab)?;
        let focused = tab.focused_surface;
        tab.layout_mut()
            .find_leaf_mut(focused)?
            .as_any_mut()
            .downcast_mut::<crate::model::ImagePanel>()
    }

    /// Find an image panel by its surface ID across all workspaces (mutable).
    /// Used by IPC handlers that target a specific surface — focus-independent.
    pub fn image_panel_mut(&mut self, surface_id: u32) -> Option<&mut crate::model::ImagePanel> {
        let (ws_idx, pid) = self.find_workspace_index_for_surface(surface_id)?;
        let workspace = self.engine.workspaces.get_mut(ws_idx)?;
        let pane = workspace.pane_layout_mut().find_pane_mut(pid)?;
        for tab in &mut pane.tabs {
            if tab.contains_surface(surface_id) {
                return tab
                    .layout_mut()
                    .find_leaf_mut(surface_id)?
                    .as_any_mut()
                    .downcast_mut::<crate::model::ImagePanel>();
            }
        }
        None
    }

    /// Refresh the cached display name of the tab containing a given surface ID.
    pub fn refresh_tab_display_name(&mut self, surface_id: u32) {
        for workspace in &mut self.engine.workspaces {
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

    /// Find the pane ID that contains a given surface ID.
    pub fn find_pane_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.engine.workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some(pid);
                        }
                    }
                }
            }
        }
        None
    }

    /// Find the workspace index containing a given pane ID.
    pub fn find_workspace_index_for_pane(&self, pane_id: u32) -> Option<usize> {
        for (i, workspace) in self.engine.workspaces.iter().enumerate() {
            if workspace.pane_layout().find_pane(pane_id).is_some() {
                return Some(i);
            }
        }
        None
    }

    /// Find a pane by ID across all workspaces (immutable).
    pub fn find_pane_by_id(&self, pane_id: u32) -> Option<&crate::model::Pane> {
        for workspace in &self.engine.workspaces {
            if let Some(pane) = workspace.pane_layout().find_pane(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    /// Find the pane ID containing a given tab ID.
    pub fn find_pane_for_tab(&self, tab_id: u32) -> Option<u32> {
        for workspace in &self.engine.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    if pane.tabs.iter().any(|t| t.id == tab_id) {
                        return Some(pid);
                    }
                }
            }
        }
        None
    }

    /// Find a pane by ID across all workspaces (mutable).
    pub fn find_pane_by_id_mut(&mut self, pane_id: u32) -> Option<&mut crate::model::Pane> {
        for workspace in &mut self.engine.workspaces {
            if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    /// Find the workspace index and pane ID containing a given surface ID.
    pub fn find_workspace_index_for_surface(&self, surface_id: u32) -> Option<(usize, u32)> {
        for (i, workspace) in self.engine.workspaces.iter().enumerate() {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some((i, pid));
                        }
                    }
                }
            }
        }
        None
    }

    /// Find a terminal by its surface ID across all workspaces (immutable).
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.engine.find_terminal_by_id(surface_id)
    }

    /// Find a terminal by its surface ID across all workspaces (mutable).
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        self.engine.find_terminal_by_id_mut(surface_id)
    }

    /// Get the focused pane ID.
    pub fn focused_pane_id(&self) -> crate::model::PaneId {
        self.active_workspace().focused_pane
    }

    /// Collect events from all terminals in ALL workspaces (not just active).
    /// Each event includes the surface_id that generated it.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for workspace in &mut self.engine.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, terminal| {
                    let mut events = terminal.take_events();
                    for event in &mut events {
                        event.surface_id = sid;
                    }
                    all_events.extend(events);
                });
        }
        all_events
    }
}
