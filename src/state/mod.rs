mod claude;
pub mod claude_error;
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

use crate::engine_state::EngineState;
use crate::model::{LogicalPx, PhysicalPx};
use crate::settings_ui::SettingsUiState;
use tasty_terminal::{Terminal, TerminalEvent, Waker};

/// Type of the currently focused surface, used for keyboard routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedSurfaceType {
    Terminal,
    Explorer,
    Markdown,
    Html,
    Image,
    Empty,
    None,
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

#[derive(Debug, Clone)]
pub struct ClaudeChildEntry {
    pub child_surface_id: u32,
    pub index: u32,
    pub cwd: Option<String>,
    pub role: Option<String>,
    pub nickname: Option<String>,
}

// IdGenerator is now in engine_state.rs

pub struct AppState {
    /// Engine-level shared state (workspaces, terminals, settings, hooks, claude, etc.)
    pub engine: EngineState,

    // ── Window-level UI state ──
    pub active_workspace: usize,
    /// Whether the settings window is open.
    pub settings_open: bool,
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
    /// Explorer folder right-click: Bookmark add/remove
    ExplorerFolder {
        surface_id: u32,
        path: String,
        is_bookmarked: bool,
        x: f32,
        y: f32,
    },
    /// Bookmark item right-click: Remove / Navigate
    BookmarkItem {
        path: String,
        name: String,
        x: f32,
        y: f32,
    },
    /// Workspace right-click in sidebar: Rename title / Rename subtitle
    Workspace {
        ws_idx: usize,
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
    /// Bookmark name input: (pane_id, path, name_buffer)
    pub bookmark_input: Option<(u32, String, String)>,
    /// Clipboard viewer popup/surface 공유 상태.
    pub clipboard_viewer: crate::clipboard_viewer_ui::ClipboardViewerState,
    /// Tab drag-and-drop state.
    pub tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub ws_drag: Option<WsDragState>,
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
            bookmark_input: None,
            clipboard_viewer: crate::clipboard_viewer_ui::ClipboardViewerState::default(),
            tab_drag: None,
            ws_drag: None,
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
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            dialogs: DialogState::new(),
            tab_bar_height: PhysicalPx(24.0),
            captured_double_tap: None,
            pending_surface_keys: Vec::new(),
            popup_hovered: false,
            recent_files: crate::recent_files::RecentFiles::load(),
            popups: {
                let mut pm = crate::ui::PopupManager::new();
                for def in crate::ui::popup_defs::all_defs() {
                    pm.register_def(def);
                }
                pm
            },
            toasts: crate::ui::ToastManager::new(),
        })
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_input_dialog_open(&self) -> bool {
        self.dialogs.has_text_input_open()
    }

    /// Returns true if any egui overlay is visible.
    pub fn has_egui_overlay_open(&self) -> bool {
        self.settings_open || self.dialogs.has_any_overlay() || self.popups.has_any_open()
    }

    /// Clean up all state associated with a closed surface:
    /// Claude agent relationships, parent tracking, and surface metadata.
    pub(crate) fn cleanup_surface(&mut self, surface_id: u32) {
        self.unregister_child(surface_id);
        self.mark_parent_closed(surface_id);
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
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
        if surface.as_terminal_surface().is_some() {
            return FocusedSurfaceType::Terminal;
        }
        if surface.as_explorer().is_some() {
            return FocusedSurfaceType::Explorer;
        }
        if surface.as_markdown().is_some() {
            return FocusedSurfaceType::Markdown;
        }
        if surface.as_html().is_some() {
            return FocusedSurfaceType::Html;
        }
        if surface.as_image().is_some() {
            return FocusedSurfaceType::Image;
        }
        if surface.as_empty_surface().is_some() {
            return FocusedSurfaceType::Empty;
        }
        FocusedSurfaceType::None
    }

    /// Explorer에서 선택된 파일 경로들을 줄바꿈으로 결합하여 반환.
    pub fn focused_explorer_selected_paths(&self) -> String {
        let pane = match self.focused_pane() {
            Some(p) => p,
            None => return String::new(),
        };
        let tab = match pane.tabs.get(pane.active_tab) {
            Some(t) => t,
            None => return String::new(),
        };
        let surface = match tab.layout().find_surface(tab.focused_surface) {
            Some(s) => s,
            None => return String::new(),
        };
        let explorer = match surface.as_explorer() {
            Some(e) => e,
            None => return String::new(),
        };
        explorer
            .selected_files
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Explorer: 선택된 파일을 OS 파일 클립보드에 복사. 성공 시 true.
    pub fn explorer_file_copy(&self) -> bool {
        let explorer = match self.focused_explorer() {
            Some(e) => e,
            None => return false,
        };
        if explorer.selected_files.is_empty() {
            return false;
        }
        let paths: Vec<&str> = explorer.selected_files.iter().map(|s| s.as_str()).collect();
        match crate::file_clipboard::set_file_clipboard(
            &paths,
            crate::file_clipboard::FileClipboardOp::Copy,
        ) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("file copy failed: {e}");
                false
            }
        }
    }

    /// Explorer: 선택된 파일을 잘라내기 (OS 파일 클립보드). 성공 시 true.
    pub fn explorer_file_cut(&self) -> bool {
        let explorer = match self.focused_explorer() {
            Some(e) => e,
            None => return false,
        };
        if explorer.selected_files.is_empty() {
            return false;
        }
        let paths: Vec<&str> = explorer.selected_files.iter().map(|s| s.as_str()).collect();
        match crate::file_clipboard::set_file_clipboard(
            &paths,
            crate::file_clipboard::FileClipboardOp::Cut,
        ) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("file cut failed: {e}");
                false
            }
        }
    }

    /// Explorer: OS 파일 클립보드에서 파일 붙여넣기.
    pub fn explorer_file_paste(&mut self) {
        let dest_dir = {
            let explorer = match self.focused_explorer() {
                Some(e) => e,
                None => return,
            };
            crate::explorer_ui::paste_destination_for(explorer)
        };
        if let Ok(Some((sources, op))) = crate::file_clipboard::get_file_clipboard() {
            for src in &sources {
                let file_name = std::path::Path::new(src)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let dest = std::path::Path::new(&dest_dir).join(&file_name);
                if op == crate::file_clipboard::FileClipboardOp::Cut {
                    if let Err(e) = std::fs::rename(src, &dest) {
                        tracing::warn!("file move failed: {e}");
                    }
                } else if std::path::Path::new(src).is_dir() {
                    if let Err(e) =
                        crate::explorer_ui::copy_dir_recursive_pub(src, &dest.to_string_lossy())
                    {
                        tracing::warn!("dir copy failed: {e}");
                    }
                } else {
                    if let Err(e) = std::fs::copy(src, &dest) {
                        tracing::warn!("file copy failed: {e}");
                    }
                }
            }
            // Refresh explorer
            if let Some(explorer) = self.focused_explorer_mut() {
                crate::model::ExplorerPanel::load_directory(&mut explorer.root_node);
            }
        }
    }

    /// Explorer: 전체 선택.
    pub fn explorer_select_all(&mut self) {
        if let Some(explorer) = self.focused_explorer_mut() {
            let mut visible = Vec::new();
            crate::explorer_ui::collect_visible_paths_pub(&explorer.root_node, &mut visible);
            explorer.select_all(&visible);
        }
    }

    fn focused_explorer(&self) -> Option<&crate::model::ExplorerPanel> {
        let pane = self.focused_pane()?;
        let tab = pane.tabs.get(pane.active_tab)?;
        let surface = tab.layout().find_surface(tab.focused_surface)?;
        surface.as_explorer()
    }

    fn focused_explorer_mut(&mut self) -> Option<&mut crate::model::ExplorerPanel> {
        let pane = self.focused_pane_mut()?;
        let tab = pane.tabs.get_mut(pane.active_tab)?;
        let focused = tab.focused_surface;
        let leaf = tab.layout_mut().find_leaf_mut(focused)?;
        leaf.as_explorer_mut()
    }

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
