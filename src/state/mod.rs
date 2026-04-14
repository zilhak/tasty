mod workspace;
mod tab;
mod pane;
mod focus;
mod claude;
mod message;
mod layout;
mod mouse;
mod mark;
mod restore;
#[cfg(test)]
mod tests;

use crate::engine_state::EngineState;
use crate::settings_ui::SettingsUiState;
use tasty_terminal::{Terminal, TerminalEvent, Waker};

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
    pub sidebar_width: f32,
    /// Sidebar visibility: false = completely hidden.
    pub sidebar_visible: bool,
    /// Sidebar collapsed: true = compact mode (narrow width, icons only).
    pub sidebar_collapsed: bool,
    /// All transient dialog/popup state.
    pub dialogs: DialogState,
    /// Measured tab bar height in physical pixels, updated each frame by egui.
    pub tab_bar_height: f32,
    /// Popup manager for internal popups (notification panel, etc.).
    pub popups: crate::ui::PopupManager,
    /// Double-tap modifier captured from winit events, for the keybinding recorder to consume.
    pub captured_double_tap: Option<String>,
}

/// A pending native context menu request.
#[derive(Clone)]
pub enum PendingNativeMenu {
    /// Tab right-click: Rename / Close
    Tab { pane_id: u32, tab_index: usize, x: f32, y: f32 },
    /// Pane/empty area right-click: Open Markdown... / Open Explorer / Open HTML...
    Pane { pane_id: u32, x: f32, y: f32 },
}

/// All transient UI dialog/popup state, grouped to avoid AppState bloat.
/// New dialogs should be added here, not as top-level AppState fields.
pub struct DialogState {
    /// Workspace rename: (workspace_index, field, edit_buffer)
    pub ws_rename: Option<(usize, WsRenameField, String)>,
    /// Tab rename: (pane_id, tab_index, edit_buffer)
    pub tab_rename: Option<(u32, usize, String)>,
    /// Markdown file path input: (pane_id, path_buffer)
    pub markdown_path: Option<(u32, String)>,
    /// Convert to markdown: target surface id
    pub markdown_convert_surface_id: Option<u32>,
    /// Surface convert popup: surface_id
    pub convert_popup: Option<u32>,
    /// HTML URL input: (pane_id, url_buffer)
    pub html_url: Option<(u32, String)>,
    /// Convert to html: target surface id
    pub html_convert_surface_id: Option<u32>,
    /// Pending native context menu
    pub pending_native_menu: Option<PendingNativeMenu>,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            ws_rename: None,
            tab_rename: None,
            markdown_path: None,
            markdown_convert_surface_id: None,
            convert_popup: None,
            html_url: None,
            html_convert_surface_id: None,
            pending_native_menu: None,
        }
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_text_input_open(&self) -> bool {
        self.html_url.is_some()
            || self.markdown_path.is_some()
            || self.ws_rename.is_some()
            || self.tab_rename.is_some()
    }

    /// Returns true if any dialog/popup overlay is open.
    pub fn has_any_overlay(&self) -> bool {
        self.convert_popup.is_some()
            || self.has_text_input_open()
    }
}



/// Which workspace field is being renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsRenameField {
    Name,
    Subtitle,
}

impl AppState {
    /// Creates initial state with one workspace, one pane, one tab, one terminal.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let engine = EngineState::new(cols, rows, waker)?;
        let sidebar_width = engine.settings.appearance.sidebar_width;
        Ok(Self {
            engine,
            active_workspace: 0,
            settings_open: false,
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            dialogs: DialogState::new(),
            tab_bar_height: 24.0,
            captured_double_tap: None,
            popups: {
                let mut pm = crate::ui::PopupManager::new();
                pm.register(crate::ui::PopupState::new(
                    "notifications",
                    "Notifications",
                    egui::vec2(350.0, 400.0),
                ));
                pm
            },
        })
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_input_dialog_open(&self) -> bool {
        self.dialogs.has_text_input_open()
    }

    /// Returns true if any egui overlay is visible.
    pub fn has_egui_overlay_open(&self) -> bool {
        self.settings_open
            || self.dialogs.has_any_overlay()
            || self.popups.has_any_open()
    }

    /// Clean up all state associated with a closed surface:
    /// Claude agent relationships, parent tracking, and surface metadata.
    pub(crate) fn cleanup_surface(&mut self, surface_id: u32) {
        self.unregister_child(surface_id);
        self.mark_parent_closed(surface_id);
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
    }

    pub fn active_workspace(&self) -> &crate::model::Workspace {
        let idx = self.active_workspace.min(self.engine.workspaces.len().saturating_sub(1));
        &self.engine.workspaces[idx]
    }

    pub fn active_workspace_mut(&mut self) -> &mut crate::model::Workspace {
        let idx = self.active_workspace.min(self.engine.workspaces.len().saturating_sub(1));
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

    /// Get the focused surface ID (the terminal that currently receives input).
    pub fn focused_surface_id(&self) -> Option<u32> {
        let pane = self.focused_pane()?;
        let panel = pane.active_panel()?;
        panel.focused_surface_id()
    }

    /// Record that the user typed on the given surface (updates last_key_input timestamp).
    pub fn record_typing(&mut self, surface_id: u32) {
        self.engine.last_key_input.insert(surface_id, std::time::Instant::now());
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

    /// Get the working directory to inherit from the focused terminal, if enabled.
    pub(crate) fn resolve_inherit_cwd(&self) -> Option<std::path::PathBuf> {
        if !self.engine.settings.general.inherit_cwd || self.engine.workspaces.is_empty() {
            return None;
        }
        // Try terminal CWD first
        if let Some(cwd) = self.focused_terminal().and_then(|t| t.get_cwd()) {
            return Some(cwd);
        }
        // Fall back to non-terminal panel paths
        let panel = self.focused_pane()?.active_panel()?;
        match panel {
            crate::model::Panel::Explorer(exp) => Some(std::path::PathBuf::from(&exp.root_path)),
            crate::model::Panel::Markdown(md) => {
                std::path::Path::new(&md.file_path).parent().map(|p| p.to_path_buf())
            }
            _ => None,
        }
    }

    /// Get the working directory to inherit from a specific surface, if enabled.
    pub(crate) fn resolve_inherit_cwd_from_surface(&self, surface_id: u32) -> Option<std::path::PathBuf> {
        if !self.engine.settings.general.inherit_cwd {
            return None;
        }
        self.engine.find_terminal_by_id(surface_id)?.get_cwd()
    }

    /// Get the ultimately focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        self.focused_pane().and_then(|p| p.active_terminal())
    }

    /// Get the ultimately focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.focused_pane_mut().and_then(|p| p.active_terminal_mut())
    }

    /// Find the pane ID that contains a given surface ID.
    pub fn find_pane_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.engine.workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if let Some(panel) = tab.panel_if_initialized() {
                            if panel.contains_surface(surface_id) {
                                return Some(pid);
                            }
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
                    // Check all tabs for this surface (including non-terminal)
                    for tab in &pane.tabs {
                        if let Some(panel) = tab.panel_if_initialized() {
                            if panel.contains_surface(surface_id) {
                                return Some((i, pid));
                            }
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
            workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
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
