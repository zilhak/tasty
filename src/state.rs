use std::collections::{HashMap, HashSet};

use tasty_hooks::HookManager;
use crate::engine_state::EngineState;
use crate::global_hooks::GlobalHookManager;
use crate::model::{DividerInfo, FocusDirection, PaneId, Rect, SplitDirection, Workspace};
use crate::notification::NotificationStore;
use crate::settings::Settings;
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
    /// Whether the notification panel overlay is open.
    pub notification_panel_open: bool,
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
    /// Workspace rename dialog state: (workspace_index, field, edit_buffer)
    pub ws_rename: Option<(usize, WsRenameField, String)>,
    /// Pane right-click context menu state: (pane_id, logical_x, logical_y).
    pub pane_context_menu: Option<PaneContextMenu>,
    /// Markdown file path dialog state: (pane_id, path_buffer).
    pub markdown_path_dialog: Option<(u32, String)>,
    /// Measured tab bar height in physical pixels, updated each frame by egui.
    pub tab_bar_height: f32,
}


/// State for the pane right-click context menu.
#[derive(Debug, Clone)]
pub struct PaneContextMenu {
    pub pane_id: u32,
    pub x: f32,
    pub y: f32,
    /// Set to true after the first egui frame where no mouse button is pressed.
    /// Until then, clicks are ignored (to avoid the opening right-click release
    /// from immediately closing the menu).
    pub armed: bool,
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
            notification_panel_open: false,
            settings_open: false,
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            ws_rename: None,
            pane_context_menu: None,
            markdown_path_dialog: None,
            tab_bar_height: 24.0,
        })
    }

    pub fn active_workspace(&self) -> &Workspace {
        let idx = self.active_workspace.min(self.engine.workspaces.len().saturating_sub(1));
        &self.engine.workspaces[idx]
    }

    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
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
        match panel {
            crate::model::Panel::Terminal(node) => Some(node.id),
            crate::model::Panel::SurfaceGroup(group) => Some(group.focused_surface),
            crate::model::Panel::Markdown(_) | crate::model::Panel::Explorer(_) => None,
        }
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
    fn send_fast_init(&mut self, surface_id: u32) {
        self.engine.send_fast_init(surface_id);
    }

    /// Get the ultimately focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        self.focused_pane().and_then(|p| p.active_terminal())
    }

    /// Get the ultimately focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.focused_pane_mut().and_then(|p| p.active_terminal_mut())
    }

    /// Add a new workspace with one pane, one tab, one terminal.
    pub fn add_workspace(&mut self) -> anyhow::Result<()> {
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let shell = if self.engine.settings.general.shell.is_empty() { None } else { Some(self.engine.settings.general.shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            self.engine.default_cols,
            self.engine.default_rows,
            pane_id,
            tab_id,
            surface_id,
            shell,
            &shell_args,
            self.engine.make_waker(surface_id),
        )?;
        self.engine.workspaces.push(ws);
        self.active_workspace = self.engine.workspaces.len() - 1;
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker)?;
        }
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a new workspace without switching to it. Used by IPC/CLI.
    /// Returns the new workspace index.
    pub fn add_workspace_background(&mut self) -> anyhow::Result<usize> {
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let shell = if self.engine.settings.general.shell.is_empty() { None } else { Some(self.engine.settings.general.shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            self.engine.default_cols,
            self.engine.default_rows,
            pane_id,
            tab_id,
            surface_id,
            shell,
            &shell_args,
            self.engine.make_waker(surface_id),
        )?;
        self.engine.workspaces.push(ws);
        let idx = self.engine.workspaces.len() - 1;
        // Do NOT change self.active_workspace
        self.send_fast_init(surface_id);
        Ok(idx)
    }

    /// Add a new tab in the focused pane without switching to it. Used by IPC/CLI.
    pub fn add_tab_background(&mut self) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_background_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker)?;
        }
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a Markdown viewer tab in the focused pane.
    pub fn add_markdown_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface(); // reuse surface id counter
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_markdown_tab(tab_id, panel_id, file_path);
        }
        Ok(())
    }

    /// Add a file explorer tab in the focused pane.
    pub fn add_explorer_tab(&mut self, root_path: String) -> anyhow::Result<()> {
        let tab_id = self.engine.next_ids.next_tab();
        let panel_id = self.engine.next_ids.next_surface();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_explorer_tab(tab_id, panel_id, root_path);
        }
        Ok(())
    }

    /// Split the focused pane into two (new independent tab bar).
    pub fn split_pane(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;

        // TODO(lazy_pty_init): If performance.lazy_pty_init is enabled,
        // create pane without spawning PTY process. Spawn on first focus.
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.engine.make_waker(new_surface_id))?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        // Focus the new pane
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        Ok(())
    }

    /// Split within the current tab (SurfaceGroup). Appears as one tab.
    pub fn split_surface(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(new_surface_id);
        if let Some(pane) = self.focused_pane_mut() {
            pane.split_active_surface_with_shell(direction, new_surface_id, cols, rows, shell_ref, &shell_args, waker)?;
        }
        self.send_fast_init(new_surface_id);
        Ok(())
    }

    /// Split a pane group with cross-workspace target support. Does NOT move focus.
    /// Returns (new_pane_id, new_surface_id).
    pub fn split_pane_targeted(
        &mut self,
        target_pane_id: Option<u32>,
        direction: SplitDirection,
    ) -> anyhow::Result<(u32, u32)> {
        let (ws_idx, resolved_pane_id) = match target_pane_id {
            Some(pid) => {
                let ws_idx = self.find_workspace_index_for_pane(pid)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pid))?;
                (ws_idx, pid)
            }
            None => {
                let ws = &self.engine.workspaces[self.active_workspace];
                (self.active_workspace, ws.focused_pane)
            }
        };

        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane = crate::model::Pane::new_with_shell(
            new_pane_id, new_tab_id, new_surface_id, cols, rows,
            shell_ref, &shell_args, self.engine.make_waker(new_surface_id),
        )?;

        let ws = &mut self.engine.workspaces[ws_idx];
        ws.pane_layout_mut()
            .split_pane_in_place(resolved_pane_id, direction, new_pane);
        // Do NOT change ws.focused_pane

        self.send_fast_init(new_surface_id);
        Ok((new_pane_id, new_surface_id))
    }

    /// Split a surface with cross-workspace target support. Does NOT move focus.
    /// Returns new_surface_id.
    pub fn split_surface_targeted(
        &mut self,
        target_surface_id: Option<u32>,
        direction: SplitDirection,
    ) -> anyhow::Result<u32> {
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(new_surface_id);

        match target_surface_id {
            Some(sid) => {
                let (ws_idx, pane_id) = self.find_workspace_index_for_surface(sid)
                    .ok_or_else(|| anyhow::anyhow!("surface {} not found", sid))?;
                let ws = &mut self.engine.workspaces[ws_idx];
                let pane = ws.pane_layout_mut().find_pane_mut(pane_id)
                    .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;
                pane.split_surface_by_id_with_shell(
                    sid, direction, new_surface_id, cols, rows,
                    shell_ref, &shell_args, waker,
                )?;
            }
            None => {
                if let Some(pane) = self.focused_pane_mut() {
                    pane.split_active_surface_with_shell(
                        direction, new_surface_id, cols, rows,
                        shell_ref, &shell_args, waker,
                    )?;
                }
            }
        }

        self.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    }

    /// Close the active tab in the focused pane. Returns true if a tab was closed.
    pub fn close_active_tab(&mut self) -> bool {
        // Collect surface IDs in the active tab before closing
        let mut surface_ids = Vec::new();
        if let Some(pane) = self.focused_pane_mut() {
            let active = pane.active_tab;
            if let Some(tab) = pane.tabs.get_mut(active) {
                tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
                    surface_ids.push(sid);
                });
            }
        }
        let closed = if let Some(pane) = self.focused_pane_mut() {
            pane.close_active_tab()
        } else {
            false
        };
        if closed {
            for sid in surface_ids {
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
        }
        closed
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self) -> bool {
        let ws = self.active_workspace_mut();
        let target_id = ws.focused_pane;

        // Collect all surface IDs in the pane being closed for claude cleanup
        let mut surface_ids = Vec::new();
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(target_id) {
            for tab in &mut pane.tabs {
                tab.panel_mut().for_each_terminal_mut(&mut |sid, _| {
                    surface_ids.push(sid);
                });
            }
        }

        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            // Claude parent-child cleanup + surface meta cleanup
            for sid in surface_ids {
                self.unregister_child(sid);
                self.mark_parent_closed(sid);
                crate::surface_meta::SurfaceMetaStore::remove(sid);
            }
        }
        removed
    }

    /// Close the focused surface within a SurfaceGroup. Returns true if a surface was removed.
    pub fn close_active_surface(&mut self) -> bool {
        let surface_id;
        if let Some(pane) = self.focused_pane_mut() {
            if let Some(panel) = pane.active_panel_mut() {
                match panel {
                    crate::model::Panel::SurfaceGroup(group) => {
                        surface_id = group.focused_surface;
                        if !group.close_surface(surface_id) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
        // Claude parent-child cleanup + surface meta cleanup
        self.unregister_child(surface_id);
        self.mark_parent_closed(surface_id);
        crate::surface_meta::SurfaceMetaStore::remove(surface_id);
        true
    }

    /// Close a specific surface by ID. Cascades up the hierarchy:
    /// surface → tab → pane → workspace as needed.
    /// If the last workspace's last surface exits, spawns a new shell.
    pub fn close_surface_by_id(&mut self, surface_id: u32) -> bool {
        // Find which workspace and pane contain this surface
        let (ws_idx, pane_id) = match self.find_workspace_index_for_surface(surface_id) {
            Some(v) => v,
            None => return false,
        };

        // Find the tab index containing this surface
        let tab_idx;
        let surface_is_sole_in_tab;
        let can_close_surface_in_group;
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
                Some(p) => p,
                None => return false,
            };

            // Find which tab has this surface
            let mut found_tab = None;
            for (i, tab) in pane.tabs.iter().enumerate() {
                if tab.panel().find_terminal(surface_id).is_some() {
                    found_tab = Some(i);
                    break;
                }
            }
            tab_idx = match found_tab {
                Some(i) => i,
                None => return false,
            };

            // Check if the surface is the only one in this tab's panel
            match pane.tabs[tab_idx].panel() {
                crate::model::Panel::Terminal(node) if node.id == surface_id => {
                    surface_is_sole_in_tab = true;
                    can_close_surface_in_group = false;
                }
                crate::model::Panel::SurfaceGroup(group) => {
                    // Try closing within the group (fails if it's the only surface)
                    surface_is_sole_in_tab = false;
                    can_close_surface_in_group = !matches!(
                        group.layout(),
                        crate::model::SurfaceGroupLayout::Single(_)
                    ) || group.layout().find_terminal(surface_id).is_none();
                }
                _ => return false, // Markdown/Explorer panels
            }
        }

        // Case 1: Surface is within a SurfaceGroup with multiple surfaces
        if !surface_is_sole_in_tab && can_close_surface_in_group {
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if let crate::model::Panel::SurfaceGroup(group) = pane.tabs[tab_idx].panel_mut() {
                if group.close_surface(surface_id) {
                    self.unregister_child(surface_id);
                    self.mark_parent_closed(surface_id);
                    crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                    return true;
                }
            }
            return false;
        }

        // Case 2: Surface is the sole content of this tab
        // Try closing the tab (fails if it's the last tab in the pane)
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if pane.tabs.len() > 1 {
                pane.tabs.remove(tab_idx);
                if pane.active_tab >= pane.tabs.len() {
                    pane.active_tab = pane.tabs.len() - 1;
                }
                self.unregister_child(surface_id);
                self.mark_parent_closed(surface_id);
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                return true;
            }
        }

        // Case 3: Last tab in pane — try closing the pane
        {
            let ws = &mut self.engine.workspaces[ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1 {
                ws.pane_layout_mut().close_pane(pane_id);
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
                self.unregister_child(surface_id);
                self.mark_parent_closed(surface_id);
                crate::surface_meta::SurfaceMetaStore::remove(surface_id);
                return true;
            }
        }

        // Case 4: Last pane in workspace — try closing the workspace
        if self.engine.workspaces.len() > 1 {
            self.engine.workspaces.remove(ws_idx);
            if self.active_workspace >= self.engine.workspaces.len() {
                self.active_workspace = self.engine.workspaces.len() - 1;
            }
            self.unregister_child(surface_id);
            self.mark_parent_closed(surface_id);
            crate::surface_meta::SurfaceMetaStore::remove(surface_id);
            return true;
        }

        // Case 5: Last workspace — respawn a new shell instead of leaving a dead surface
        self.respawn_surface(surface_id);
        true
    }

    /// Respawn a new shell in the current terminal surface (replacing the dead one).
    fn respawn_surface(&mut self, surface_id: u32) {
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = if self.engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(self.engine.settings.general.shell.clone())
        };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);

        match Terminal::new_with_shell_args(
            cols,
            rows,
            shell.as_deref(),
            &shell_args,
            surface_id,
            waker,
        ) {
            Ok(new_terminal) => {
                // Replace the terminal in the existing surface node
                if let Some(term) = self.find_terminal_by_id_mut(surface_id) {
                    *term = new_terminal;
                }
                self.send_fast_init(surface_id);
            }
            Err(e) => {
                tracing::error!("Failed to respawn shell for surface {}: {}", surface_id, e);
            }
        }
    }

    // ---- Claude parent-child management ----

    /// Get the next child index for a parent, incrementing the counter.
    pub fn next_child_index(&mut self, parent_id: u32) -> u32 {
        let idx = self.engine.claude_next_child_index.entry(parent_id).or_insert(0);
        *idx += 1;
        *idx
    }

    /// Register a child entry under a parent surface.
    pub fn register_child(&mut self, parent_id: u32, entry: ClaudeChildEntry) {
        self.engine.claude_child_parent.insert(entry.child_surface_id, parent_id);
        self.engine.claude_parent_children.entry(parent_id).or_default().push(entry);
    }

    /// Unregister a child surface. Cleans up parent tracking if parent is closed and has no more children.
    pub fn unregister_child(&mut self, child_surface_id: u32) {
        self.engine.claude_idle_state.remove(&child_surface_id);
        self.engine.claude_needs_input_state.remove(&child_surface_id);
        if let Some(parent_id) = self.engine.claude_child_parent.remove(&child_surface_id) {
            if let Some(children) = self.engine.claude_parent_children.get_mut(&parent_id) {
                children.retain(|c| c.child_surface_id != child_surface_id);
                if children.is_empty() && self.engine.claude_closed_parents.contains(&parent_id) {
                    self.engine.claude_parent_children.remove(&parent_id);
                    self.engine.claude_closed_parents.remove(&parent_id);
                    self.engine.claude_next_child_index.remove(&parent_id);
                }
            }
        }
    }

    /// Mark a parent surface as closed. If it has no children, clean up immediately.
    pub fn mark_parent_closed(&mut self, parent_surface_id: u32) {
        self.engine.claude_idle_state.remove(&parent_surface_id);
        self.engine.claude_needs_input_state.remove(&parent_surface_id);
        if self.engine.claude_parent_children.contains_key(&parent_surface_id) {
            let children_empty = self.engine.claude_parent_children
                .get(&parent_surface_id)
                .map(|c| c.is_empty())
                .unwrap_or(true);
            if children_empty {
                self.engine.claude_parent_children.remove(&parent_surface_id);
                self.engine.claude_next_child_index.remove(&parent_surface_id);
            } else {
                self.engine.claude_closed_parents.insert(parent_surface_id);
            }
        }
    }

    /// Get all children of a parent surface.
    pub fn children_of(&self, parent_id: u32) -> &[ClaudeChildEntry] {
        self.engine.claude_parent_children.get(&parent_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get the parent of a child surface.
    pub fn parent_of(&self, child_id: u32) -> Option<u32> {
        self.engine.claude_child_parent.get(&child_id).copied()
    }

    /// Set the Claude idle state for a surface. When becoming non-idle, also clears needs_input.
    pub fn set_claude_idle(&mut self, surface_id: u32, idle: bool) {
        self.engine.claude_idle_state.insert(surface_id, idle);
        if !idle {
            self.engine.claude_needs_input_state.remove(&surface_id);
        }
    }

    /// Set the Claude needs-input state for a surface.
    pub fn set_claude_needs_input(&mut self, surface_id: u32, needs_input: bool) {
        self.engine.claude_needs_input_state.insert(surface_id, needs_input);
    }

    /// Get the Claude state string for a surface: "needs_input", "idle", or "active".
    pub fn claude_state_of(&self, surface_id: u32) -> &str {
        if self.engine.claude_needs_input_state.get(&surface_id).copied().unwrap_or(false) {
            "needs_input"
        } else if self.engine.claude_idle_state.get(&surface_id).copied().unwrap_or(false) {
            "idle"
        } else {
            "active"
        }
    }

    /// Split the focused pane and return the new surface ID.
    /// This is like `split_pane` but returns the new surface_id for callers that need it.
    pub fn split_pane_get_surface(&mut self, direction: SplitDirection) -> anyhow::Result<u32> {
        let new_pane_id = self.engine.next_ids.next_pane();
        let new_tab_id = self.engine.next_ids.next_tab();
        let new_surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;

        let shell = self.engine.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.engine.make_waker(new_surface_id))?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        self.send_fast_init(new_surface_id);
        Ok(new_surface_id)
    }

    /// Close a specific pane by its ID (across the active workspace).
    /// Returns true if the pane was found and removed.
    pub fn close_pane_by_id(&mut self, pane_id: u32) -> bool {
        let ws = self.active_workspace_mut();
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if ws.focused_pane == pane_id {
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
            }
        }
        removed
    }

    /// Find the pane ID that contains a given surface ID.
    pub fn find_pane_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.engine.workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    if pane.find_terminal(surface_id).is_some() {
                        return Some(pid);
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

    /// Find the workspace index and pane ID containing a given surface ID.
    pub fn find_workspace_index_for_surface(&self, surface_id: u32) -> Option<(usize, u32)> {
        for (i, workspace) in self.engine.workspaces.iter().enumerate() {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    if pane.find_terminal(surface_id).is_some() {
                        return Some((i, pid));
                    }
                }
            }
        }
        None
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.engine.workspaces.len() {
            self.active_workspace = index;
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (claude parent-child, surface meta) in the workspace.
    pub fn close_active_workspace(&mut self) -> bool {
        if self.engine.workspaces.is_empty() {
            return false;
        }
        let ws_idx = self.active_workspace;
        // Collect all surface IDs for cleanup
        let surface_ids = self.engine.workspaces[ws_idx].all_surface_ids();
        self.engine.workspaces.remove(ws_idx);
        // Adjust active workspace index
        if self.active_workspace >= self.engine.workspaces.len() && !self.engine.workspaces.is_empty() {
            self.active_workspace = self.engine.workspaces.len() - 1;
        }
        // Cleanup
        for sid in surface_ids {
            self.unregister_child(sid);
            self.mark_parent_closed(sid);
            crate::surface_meta::SurfaceMetaStore::remove(sid);
        }
        true
    }

    /// Ensure at least one workspace exists. If none exist, create a new one.
    /// Returns true if a new workspace was created.
    pub fn ensure_workspace_exists(&mut self) -> bool {
        if !self.engine.workspaces.is_empty() {
            return false;
        }
        match self.add_workspace() {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("Failed to create workspace: {}", e);
                false
            }
        }
    }

    /// Next tab in the focused pane.
    pub fn next_tab_in_pane(&mut self) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.next_tab();
        }
    }

    /// Previous tab in the focused pane.
    pub fn prev_tab_in_pane(&mut self) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.prev_tab();
        }
    }

    /// Go to tab by index (0-based) in the focused pane.
    pub fn goto_tab_in_pane(&mut self, index: usize) -> bool {
        if let Some(pane) = self.focused_pane_mut() {
            pane.goto_tab(index)
        } else {
            false
        }
    }

    /// Move focus forward: within the active SurfaceGroup first, then between panes.
    pub fn move_focus_forward(&mut self) {
        let ws = self.active_workspace_mut();
        let pane_id = ws.focused_pane;

        // Try to move within a SurfaceGroup first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    if group.layout().all_surface_ids().len() > 1 {
                        group.move_focus_forward();
                        return;
                    }
                }
            }
        }

        // Not in a multi-surface group, move between panes
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

    /// Move focus backward: within the active SurfaceGroup first, then between panes.
    pub fn move_focus_backward(&mut self) {
        let ws = self.active_workspace_mut();
        let pane_id = ws.focused_pane;

        // Try to move within a SurfaceGroup first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    if group.layout().all_surface_ids().len() > 1 {
                        group.move_focus_backward();
                        return;
                    }
                }
            }
        }

        // Not in a multi-surface group, move between panes
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().prev_pane_id(ws.focused_pane);
    }

    /// Move focus in a spatial direction (left/right/up/down).
    /// First tries to move within a SurfaceGroup, then moves between panes.
    pub fn move_focus_direction(&mut self, direction: FocusDirection) {
        let ws = self.active_workspace_mut();
        let pane_id = ws.focused_pane;

        // Try to move within a SurfaceGroup first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    if let Some(new_surface_id) = group.directional_focus(direction) {
                        group.focused_surface = new_surface_id;
                        return;
                    }
                }
            }
        }

        // Try to move between panes
        let ws = self.active_workspace_mut();
        if let Some(target_pane_id) = ws.pane_layout().directional_focus(ws.focused_pane, direction) {
            ws.focused_pane = target_pane_id;
        }
    }

    /// Move focus to the next pane only (skip surface group logic).
    pub fn move_pane_focus_forward(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

    /// Move focus to the previous pane only (skip surface group logic).
    pub fn move_pane_focus_backward(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().prev_pane_id(ws.focused_pane);
    }

    /// Move focus to the next surface within the current pane's SurfaceGroup.
    /// Does nothing if not in a multi-surface group.
    pub fn move_surface_focus_forward(&mut self) {
        let ws = self.active_workspace_mut();
        let pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    group.move_focus_forward();
                }
            }
        }
    }

    /// Move focus to the previous surface within the current pane's SurfaceGroup.
    /// Does nothing if not in a multi-surface group.
    pub fn move_surface_focus_backward(&mut self) {
        let ws = self.active_workspace_mut();
        let pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    group.move_focus_backward();
                }
            }
        }
    }

    /// Process all terminals in ALL workspaces to drain PTY channels.
    /// Returns true if the active workspace had any changes (for redraw).
    pub fn process_all(&mut self) -> bool {
        let active_idx = self.active_workspace;
        let mut active_changed = false;
        for (i, workspace) in self.engine.workspaces.iter_mut().enumerate() {
            let changed = workspace.pane_layout_mut().process_all();
            if i == active_idx {
                active_changed = changed;
            }
        }
        active_changed
    }

    /// Compute all render regions for the active workspace.
    /// Returns: for each pane, the pane rect and the terminal regions within it.
    pub fn render_regions(
        &self,
        terminal_rect: Rect,
    ) -> Vec<(PaneId, Rect, Vec<(u32, &Terminal, Rect)>)> {
        let ws = self.active_workspace();
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        let mut result = Vec::new();
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                // Reserve space for tab bar at top of each pane
                let tab_bar_h = self.tab_bar_height;
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                let regions = match pane.active_panel() {
                    Some(panel) => panel.render_regions(content_rect),
                    None => Vec::new(),
                };
                result.push((pane_id, pane_rect, regions));
            }
        }
        result
    }

    /// Get the actual content rect for the focused surface (accounting for tab bar).
    /// Returns None if no surface is focused.
    pub fn focused_surface_rect(&self, terminal_rect: Rect) -> Option<Rect> {
        let surface_id = self.focused_surface_id()?;
        let regions = self.render_regions(terminal_rect);
        for (_pane_id, _pane_rect, terminal_regions) in &regions {
            for (sid, _term, rect) in terminal_regions {
                if *sid == surface_id {
                    return Some(*rect);
                }
            }
        }
        None
    }

    /// Find the surface ID at the given physical pixel position.
    pub fn surface_id_at_position(&self, x: f32, y: f32, terminal_rect: Rect) -> Option<u32> {
        let regions = self.render_regions(terminal_rect);
        for (_pane_id, _pane_rect, terminal_regions) in &regions {
            for (sid, _term, rect) in terminal_regions {
                if rect.contains(x, y) {
                    return Some(*sid);
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

    /// Set the focused pane in the active workspace to the given pane_id.
    /// Returns true if the pane exists.
    pub fn focus_pane(&mut self, pane_id: u32) -> bool {
        let ws = self.active_workspace_mut();
        if ws.pane_layout().find_pane(pane_id).is_some() {
            ws.focused_pane = pane_id;
            true
        } else {
            false
        }
    }

    /// Find which pane contains the surface, focus that pane, and if it's in a SurfaceGroup,
    /// focus that surface. Returns true if found.
    pub fn focus_surface(&mut self, surface_id: u32) -> bool {
        // Find the pane containing the surface in the active workspace.
        let ws = self.active_workspace_mut();
        let pane_ids = ws.pane_layout().all_pane_ids();
        let mut found_pane_id = None;
        for pid in pane_ids {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                if pane.find_terminal(surface_id).is_some() {
                    found_pane_id = Some(pid);
                    break;
                }
            }
        }
        let pane_id = match found_pane_id {
            Some(id) => id,
            None => return false,
        };
        // Focus the pane.
        let ws = self.active_workspace_mut();
        ws.focused_pane = pane_id;
        // If the active panel for that pane is a SurfaceGroup, focus the surface within it.
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(panel) = pane.active_panel_mut() {
                if let crate::model::Panel::SurfaceGroup(group) = panel {
                    if group.layout().find_terminal(surface_id).is_some() {
                        group.focused_surface = surface_id;
                    }
                }
            }
        }
        true
    }

    /// Update stored grid dimensions.
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.engine.default_cols = cols;
        self.engine.default_rows = rows;
    }

    /// Resize all terminals in the active workspace to match a given terminal rect.
    pub fn resize_all(&mut self, terminal_rect: Rect, cell_width: f32, cell_height: f32) {
        let tab_bar_h = self.tab_bar_height;
        let ws = self.active_workspace_mut();
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                if let Some(panel) = pane.active_panel_mut() {
                    panel.resize_all(content_rect, cell_width, cell_height);
                }
            }
        }
    }

    /// Get the focused pane ID.
    pub fn focused_pane_id(&self) -> PaneId {
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

    /// Set a read mark on the focused terminal (or a specific surface).
    pub fn set_mark(&mut self, surface_id: Option<u32>) {
        if let Some(target_sid) = surface_id {
            for workspace in &mut self.engine.workspaces {
                let mut found = false;
                workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
                    if sid == target_sid {
                        terminal.set_mark();
                        found = true;
                    }
                });
                if found {
                    return;
                }
            }
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.set_mark();
        }
    }

    /// Read since mark on the focused terminal (or a specific surface).
    pub fn read_since_mark(&mut self, surface_id: Option<u32>, strip_ansi: bool) -> String {
        if let Some(target_sid) = surface_id {
            let mut result = None;
            for workspace in &mut self.engine.workspaces {
                workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
                    if sid == target_sid && result.is_none() {
                        result = Some(terminal.read_since_mark(strip_ansi));
                    }
                });
                if result.is_some() {
                    break;
                }
            }
            result.unwrap_or_default()
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.read_since_mark(strip_ansi)
        } else {
            String::new()
        }
    }

    /// Determine the cursor style for the given position within the terminal rect.
    /// Returns Some(true) for terminal surfaces (I-beam), Some(false) for non-terminal
    /// panels like Explorer/Markdown (default pointer), or None if not over any pane content.
    pub fn cursor_style_at(&self, x: f32, y: f32, terminal_rect: Rect) -> Option<bool> {
        if !terminal_rect.contains(x, y) {
            return None;
        }
        let tab_bar_h = self.tab_bar_height;
        let ws = self.active_workspace();
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, rect) in &pane_rects {
            let content_rect = Rect {
                x: rect.x,
                y: rect.y + tab_bar_h,
                width: rect.width,
                height: (rect.height - tab_bar_h).max(1.0),
            };
            if content_rect.contains(x, y) {
                // Check the panel type of this pane
                if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                    return Some(match pane.active_panel() {
                        Some(crate::model::Panel::Terminal(_)) => true,
                        Some(crate::model::Panel::SurfaceGroup(_)) => true,
                        Some(crate::model::Panel::Markdown(_)) => false,
                        Some(crate::model::Panel::Explorer(_)) => false,
                        None => false,
                    });
                }
                return None;
            }
        }
        None
    }

    /// Focus the pane at the given physical pixel position within the terminal rect.
    /// Returns true if focus changed.
    pub fn focus_pane_at_position(&mut self, x: f32, y: f32, terminal_rect: Rect) -> bool {
        let ws = self.active_workspace();
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, rect) in pane_rects {
            if rect.contains(x, y) {
                let old = self.active_workspace().focused_pane;
                if old != pane_id {
                    self.active_workspace_mut().focused_pane = pane_id;
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// Focus the surface (within a SurfaceGroup) at the given physical pixel position.
    /// This should be called after focus_pane_at_position to also focus within the pane's panel.
    /// Returns true if focus changed.
    pub fn focus_surface_at_position(&mut self, x: f32, y: f32, terminal_rect: Rect) -> bool {
        let ws = self.active_workspace();
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        // Find the focused pane's rect
        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return false,
        };

        // Account for tab bar height
        let ws = self.active_workspace();
        let tab_count = ws.pane_layout().find_pane(focused_id)
            .map(|p| p.tabs.len())
            .unwrap_or(0);
        let tab_bar_h = self.tab_bar_height;
        let content_rect = Rect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(1.0),
        };

        let ws = self.active_workspace_mut();
        let pane = match ws.pane_layout_mut().find_pane_mut(focused_id) {
            Some(p) => p,
            None => return false,
        };

        let panel = match pane.active_panel_mut() {
            Some(p) => p,
            None => return false,
        };

        match panel {
            crate::model::Panel::SurfaceGroup(group) => {
                if let Some(surface_id) = group.layout().find_surface_at(x, y, content_rect) {
                    if group.focused_surface != surface_id {
                        group.focused_surface = surface_id;
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Find a pane-level divider at the given position.
    pub fn find_pane_divider_at(&self, x: f32, y: f32, terminal_rect: Rect, threshold: f32) -> Option<DividerInfo> {
        let ws = self.active_workspace();
        ws.pane_layout().find_divider_at(x, y, terminal_rect, threshold)
    }

    /// Find a surface-level divider at the given position (within the focused pane's panel).
    pub fn find_surface_divider_at(&self, x: f32, y: f32, terminal_rect: Rect, threshold: f32) -> Option<DividerInfo> {
        let ws = self.active_workspace();
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return None,
        };

        let pane = ws.pane_layout().find_pane(focused_id)?;
        let tab_bar_h = self.tab_bar_height;
        let content_rect = Rect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(1.0),
        };

        let panel = pane.active_panel()?;
        match panel {
            crate::model::Panel::SurfaceGroup(group) => {
                group.layout().find_divider_at(x, y, content_rect, threshold)
            }
            _ => None,
        }
    }

    /// Update a pane-level split ratio based on a divider drag.
    pub fn update_pane_divider(&mut self, divider: &DividerInfo, x: f32, y: f32, terminal_rect: Rect) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => (x - divider.split_rect.x) / divider.split_rect.width,
            SplitDirection::Horizontal => (y - divider.split_rect.y) / divider.split_rect.height,
        };
        let ws = self.active_workspace_mut();
        ws.pane_layout_mut().update_ratio_for_rect(divider.split_rect, new_ratio, terminal_rect)
    }

    /// Update a surface-level split ratio based on a divider drag.
    pub fn update_surface_divider(&mut self, divider: &DividerInfo, x: f32, y: f32, terminal_rect: Rect) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => (x - divider.split_rect.x) / divider.split_rect.width,
            SplitDirection::Horizontal => (y - divider.split_rect.y) / divider.split_rect.height,
        };

        let tab_bar_h = self.tab_bar_height;
        let ws = self.active_workspace_mut();
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return false,
        };

        let pane = match ws.pane_layout_mut().find_pane_mut(focused_id) {
            Some(p) => p,
            None => return false,
        };
        let content_rect = Rect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(1.0),
        };

        let panel = match pane.active_panel_mut() {
            Some(p) => p,
            None => return false,
        };

        match panel {
            crate::model::Panel::SurfaceGroup(group) => {
                group.layout_mut().update_ratio_for_rect(divider.split_rect, new_ratio, content_rect)
            }
            _ => false,
        }
    }

    /// Send a message from one surface to another. Returns the assigned message ID.
    pub fn send_message(&mut self, from: u32, to: u32, content: String) -> u32 {
        self.engine.surface_next_message_id += 1;
        let id = self.engine.surface_next_message_id;
        let msg = SurfaceMessage { id, from_surface_id: from, content };
        self.engine.surface_messages.entry(to).or_default().push(msg);
        id
    }

    /// Read (and optionally consume) messages queued for a surface.
    /// If `from` is Some, only return messages from that sender.
    /// If `peek` is false, the returned messages are removed from the queue.
    pub fn read_messages(&mut self, surface_id: u32, from: Option<u32>, peek: bool) -> Vec<SurfaceMessage> {
        let queue = match self.engine.surface_messages.get_mut(&surface_id) {
            Some(q) => q,
            None => return vec![],
        };

        if peek {
            queue
                .iter()
                .filter(|m| from.map_or(true, |f| m.from_surface_id == f))
                .cloned()
                .collect()
        } else {
            let mut retained = Vec::new();
            let mut taken = Vec::new();
            for msg in queue.drain(..) {
                if from.map_or(true, |f| msg.from_surface_id == f) {
                    taken.push(msg);
                } else {
                    retained.push(msg);
                }
            }
            *queue = retained;
            taken
        }
    }

    /// Count messages queued for a surface.
    pub fn message_count(&self, surface_id: u32) -> usize {
        self.engine.surface_messages.get(&surface_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Clear all messages queued for a surface.
    pub fn clear_messages(&mut self, surface_id: u32) {
        self.engine.surface_messages.remove(&surface_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
        AppState::new(80, 24, waker).unwrap()
    }

    /// 현재 활성 워크스페이스의 모든 surface ID를 수집한다.
    fn collect_surface_ids(state: &mut AppState) -> Vec<u32> {
        let mut ids = Vec::new();
        let ws = state.active_workspace_mut();
        ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
            ids.push(sid);
        });
        ids
    }

    /// 모든 워크스페이스에 걸쳐 surface ID를 수집한다.
    fn collect_all_surface_ids(state: &mut AppState) -> Vec<u32> {
        let mut ids = Vec::new();
        for ws in &mut state.engine.workspaces {
            ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
                ids.push(sid);
            });
        }
        ids
    }

    // ---- find_terminal_by_id ----

    #[test]
    fn find_terminal_by_id_exists() {
        let mut state = test_state();
        let surface_ids = collect_surface_ids(&mut state);
        assert!(!surface_ids.is_empty());
        let first_id = surface_ids[0];
        assert!(state.find_terminal_by_id(first_id).is_some());
    }

    #[test]
    fn find_terminal_by_id_nonexistent() {
        let state = test_state();
        assert!(state.find_terminal_by_id(9999).is_none());
    }

    #[test]
    fn find_terminal_by_id_after_split() {
        let mut state = test_state();
        let original_ids = collect_surface_ids(&mut state);
        let original_id = original_ids[0];

        state.split_pane(SplitDirection::Vertical).unwrap();

        let all_ids = collect_surface_ids(&mut state);
        assert_eq!(all_ids.len(), 2);

        assert!(state.find_terminal_by_id(original_id).is_some());
        let new_id = *all_ids.iter().find(|&&id| id != original_id).unwrap();
        assert!(state.find_terminal_by_id(new_id).is_some());
    }

    #[test]
    fn find_terminal_by_id_across_tabs() {
        let mut state = test_state();
        let original_ids = collect_surface_ids(&mut state);
        let first_id = original_ids[0];

        state.add_tab().unwrap();

        let all_ids = collect_all_surface_ids(&mut state);
        assert_eq!(all_ids.len(), 2);

        assert!(state.find_terminal_by_id(first_id).is_some());
        let second_id = *all_ids.iter().find(|&&id| id != first_id).unwrap();
        assert!(state.find_terminal_by_id(second_id).is_some());
    }

    // ---- focus_pane ----

    #[test]
    fn focus_pane_valid() {
        let mut state = test_state();
        state.split_pane(SplitDirection::Vertical).unwrap();

        let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
        assert_eq!(pane_ids.len(), 2);

        // 첫 번째 pane에 포커스
        let result = state.focus_pane(pane_ids[0]);
        assert!(result);
        assert_eq!(state.active_workspace().focused_pane, pane_ids[0]);
    }

    #[test]
    fn focus_pane_invalid() {
        let mut state = test_state();
        let result = state.focus_pane(9999);
        assert!(!result);
    }

    #[test]
    fn focus_pane_preserves_state() {
        let mut state = test_state();
        state.split_pane(SplitDirection::Vertical).unwrap();

        let ws_count_before = state.engine.workspaces.len();
        let tab_count_before = state.active_workspace().pane_layout().all_pane_ids().len();

        let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
        state.focus_pane(pane_ids[0]);

        assert_eq!(state.engine.workspaces.len(), ws_count_before);
        assert_eq!(
            state.active_workspace().pane_layout().all_pane_ids().len(),
            tab_count_before
        );
    }

    // ---- focus_surface ----

    #[test]
    fn focus_surface_valid() {
        let mut state = test_state();
        let surface_ids = collect_surface_ids(&mut state);
        let first_id = surface_ids[0];
        assert!(state.focus_surface(first_id));
    }

    #[test]
    fn focus_surface_invalid() {
        let mut state = test_state();
        assert!(!state.focus_surface(9999));
    }

    #[test]
    fn focus_surface_changes_pane_focus() {
        let mut state = test_state();

        // split 후 두 번째 pane의 surface ID를 구한다
        state.split_pane(SplitDirection::Vertical).unwrap();

        let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
        let first_pane_id = pane_ids[0];

        // 현재 포커스는 새로 생성된 두 번째 pane에 있다 (split 후 새 pane에 포커스)
        // 첫 번째 pane의 surface를 찾아 포커스한다
        let first_pane_surface: u32 = {
            let ws = state.active_workspace_mut();
            let pane = ws.pane_layout_mut().find_pane_mut(first_pane_id).unwrap();
            let mut sid = 0u32;
            for tab in &mut pane.tabs {
                tab.panel_mut().for_each_terminal_mut(&mut |id, _| {
                    sid = id;
                });
            }
            sid
        };

        assert!(state.focus_surface(first_pane_surface));
        assert_eq!(state.active_workspace().focused_pane, first_pane_id);
    }

    // ---- close operations ----

    #[test]
    fn close_active_pane_single_fails() {
        let mut state = test_state();
        assert!(!state.close_active_pane());
    }

    #[test]
    fn close_active_pane_after_split() {
        let mut state = test_state();
        state.split_pane(SplitDirection::Vertical).unwrap();

        assert_eq!(
            state.active_workspace().pane_layout().all_pane_ids().len(),
            2
        );
        assert!(state.close_active_pane());
        assert_eq!(
            state.active_workspace().pane_layout().all_pane_ids().len(),
            1
        );
    }

    #[test]
    fn close_active_tab_single_fails() {
        let mut state = test_state();
        assert!(!state.close_active_tab());
    }

    #[test]
    fn close_active_tab_after_add() {
        let mut state = test_state();
        state.add_tab().unwrap();

        let pane_id = state.active_workspace().focused_pane;
        let tab_count = state
            .active_workspace()
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs
            .len();
        assert_eq!(tab_count, 2);

        assert!(state.close_active_tab());

        let tab_count_after = state
            .active_workspace()
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs
            .len();
        assert_eq!(tab_count_after, 1);
    }

    // ---- workspace operations ----

    #[test]
    fn add_workspace_increments_count() {
        let mut state = test_state();
        assert_eq!(state.engine.workspaces.len(), 1);
        state.add_workspace().unwrap();
        assert_eq!(state.engine.workspaces.len(), 2);
    }

    #[test]
    fn switch_workspace_valid() {
        let mut state = test_state();
        state.add_workspace().unwrap();
        assert_eq!(state.active_workspace, 1);

        state.switch_workspace(0);
        assert_eq!(state.active_workspace, 0);
    }

    #[test]
    fn switch_workspace_out_of_range() {
        let mut state = test_state();
        state.switch_workspace(999);
        assert_eq!(state.active_workspace, 0);
    }

    // ---- focus movement ----

    #[test]
    fn move_focus_forward_single_pane() {
        let mut state = test_state();
        let before = state.active_workspace().focused_pane;
        state.move_focus_forward();
        let after = state.active_workspace().focused_pane;
        assert_eq!(before, after);
    }

    #[test]
    fn move_focus_forward_two_panes() {
        let mut state = test_state();
        state.split_pane(SplitDirection::Vertical).unwrap();

        let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
        assert_eq!(pane_ids.len(), 2);

        // split 후 새 pane(second)에 포커스가 있다
        let initial_focus = state.active_workspace().focused_pane;

        state.move_focus_forward();
        let after_first_move = state.active_workspace().focused_pane;
        assert_ne!(after_first_move, initial_focus);

        state.move_focus_forward();
        let after_second_move = state.active_workspace().focused_pane;
        // 두 번 이동하면 원래 위치로 돌아와야 한다
        assert_eq!(after_second_move, initial_focus);
    }
}
