mod focus;
mod layout;
mod mark;
mod message;
mod mouse;
mod pane;
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

    /// Per-surface host view state for `MarkdownPanel` (content cache, scroll, commonmark cache).
    /// `MarkdownPanel` itself only holds `file_path` + reload tracking; everything GUI-bound lives here.
    pub markdown_views: crate::ui::markdown_view::MarkdownViewStore,

    /// Per-surface host view state for `ImagePanel` (pixel buffer, textures, edit state,
    /// undo history, brush settings, popup buffers).
    pub image_views: crate::ui::image_view::ImageViewStore,

    /// Per-surface host view state for `ClipboardViewerPanel` (search query, selected
    /// index, pending clear flag). `ClipboardViewerPanel` itself only holds `id`.
    /// 별개로 popup용 단일 인스턴스는 `dialogs.clipboard_viewer`에 있음.
    pub clipboard_viewer_views: crate::clipboard_viewer_ui::ClipboardViewerViewStore,
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
    /// Clipboard viewer popup/surface 공유 상태.
    pub clipboard_viewer: crate::clipboard_viewer_ui::ClipboardViewerState,
    /// Pending file drag request (paths to drag to external apps).
    pub pending_file_drag: Option<Vec<String>>,
    /// Tab drag-and-drop state.
    pub tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub ws_drag: Option<WsDragState>,
    /// 부팅 시점 정보/에러 알림용 modal 큐. 큐 head를 [확인] 버튼으로 처리한다.
    /// `crate::ui::info_modal::show_info_modal()`로 push.
    pub info_modal_queue: VecDeque<InfoModal>,
}

/// Tab drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct TabDragState {
    pub pane_id: u32,
    pub tab_index: usize,
    /// Current mouse x in logical pixels (for insert position calculation).
    pub current_x: f32,
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
            clipboard_viewer: crate::clipboard_viewer_ui::ClipboardViewerState::default(),
            pending_file_drag: None,
            tab_drag: None,
            ws_drag: None,
            info_modal_queue: VecDeque::new(),
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
            toasts: crate::ui::ToastManager::new(),
            markdown_views: Default::default(),
            image_views: Default::default(),
            clipboard_viewer_views: Default::default(),
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

    /// Clean up all state associated with a closed surface:
    /// surface metadata and per-surface host view state.
    pub(crate) fn cleanup_surface(&mut self, surface_id: u32) {
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
        self.markdown_views.drop_view(surface_id);
        self.image_views.drop_view(surface_id);
        self.clipboard_viewer_views.drop_view(surface_id);
    }

    pub fn active_workspace(&self) -> &crate::model::Workspace {
        let idx = self
            .active_workspace
            .min(self.engine.workspaces.len().saturating_sub(1));
        &self.engine.workspaces[idx]
    }

    pub fn active_workspace_mut(&mut self) -> &mut crate::model::Workspace {
        let idx = self
            .active_workspace
            .min(self.engine.workspaces.len().saturating_sub(1));
        &mut self.engine.workspaces[idx]
    }

    /// Get the focused pane in the active workspace, or the first pane as fallback.
    pub fn focused_pane(&self) -> Option<&crate::model::Pane> {
        let ws = self.active_workspace();
        let layout = ws.pane_layout();
        layout
            .find_pane(ws.focused_pane)
            .or_else(|| layout.first_pane())
    }

    /// Get the focused pane (mutable) in the active workspace, or the first pane as fallback.
    pub fn focused_pane_mut(&mut self) -> Option<&mut crate::model::Pane> {
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
