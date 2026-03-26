use std::collections::{HashMap, HashSet};

use tasty_hooks::HookManager;
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

struct IdGenerator {
    workspace: u32,
    pane: u32,
    tab: u32,
    surface: u32,
}

impl IdGenerator {
    fn new() -> Self {
        Self {
            workspace: 1,
            pane: 1,
            tab: 1,
            surface: 1,
        }
    }

    fn next_workspace(&mut self) -> u32 {
        let id = self.workspace;
        self.workspace += 1;
        id
    }

    fn next_pane(&mut self) -> u32 {
        let id = self.pane;
        self.pane += 1;
        id
    }

    fn next_tab(&mut self) -> u32 {
        let id = self.tab;
        self.tab += 1;
        id
    }

    fn next_surface(&mut self) -> u32 {
        let id = self.surface;
        self.surface += 1;
        id
    }
}

pub struct AppState {
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    next_ids: IdGenerator,
    pub default_cols: usize,
    pub default_rows: usize,
    pub notifications: NotificationStore,
    /// Whether the notification panel overlay is open.
    pub notification_panel_open: bool,
    /// Application settings loaded from config file.
    pub settings: Settings,
    /// Whether the settings window is open.
    pub settings_open: bool,
    /// Persistent UI state for the settings window.
    pub settings_ui_state: SettingsUiState,
    /// Hook manager for surface event hooks.
    pub hook_manager: HookManager,
    /// Cached sidebar width from settings (logical pixels).
    pub sidebar_width: f32,
    /// Waker callback to wake the event loop when new PTY data arrives.
    waker: Waker,
    /// Workspace rename dialog state: (workspace_index, field, edit_buffer)
    pub ws_rename: Option<(usize, WsRenameField, String)>,
    /// Claude parent-child relationships: parent_surface_id -> children
    pub claude_parent_children: HashMap<u32, Vec<ClaudeChildEntry>>,
    /// Claude child -> parent mapping
    pub claude_child_parent: HashMap<u32, u32>,
    /// Set of parent surfaces that have been closed but still have live children
    pub claude_closed_parents: HashSet<u32>,
    /// Next child index counter per parent
    claude_next_child_index: HashMap<u32, u32>,
    /// Claude idle state per surface (true = idle)
    pub claude_idle_state: HashMap<u32, bool>,
    /// Claude needs-input state per surface (true = needs input)
    pub claude_needs_input_state: HashMap<u32, bool>,
    /// Surface message queues: target_surface_id -> messages
    pub surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    /// Next message ID counter
    surface_next_message_id: u32,
    /// Global hook manager for timer-based and file-watching hooks.
    pub global_hook_manager: GlobalHookManager,
    /// Last key input time per surface (for typing detection).
    pub last_key_input: HashMap<u32, std::time::Instant>,
    /// Pane right-click context menu state: (pane_id, logical_x, logical_y).
    pub pane_context_menu: Option<PaneContextMenu>,
    /// Markdown file path dialog state: (pane_id, path_buffer).
    pub markdown_path_dialog: Option<(u32, String)>,
}

/// State for the pane right-click context menu.
#[derive(Debug, Clone)]
pub struct PaneContextMenu {
    pub pane_id: u32,
    pub x: f32,
    pub y: f32,
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
        let settings = Settings::load();
        let mut next_ids = IdGenerator::new();
        let ws_id = next_ids.next_workspace();
        let pane_id = next_ids.next_pane();
        let tab_id = next_ids.next_tab();
        let surface_id = next_ids.next_surface();

        let shell = if settings.general.shell.is_empty() { None } else { Some(settings.general.shell.as_str()) };
        let shell_args_owned = settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(ws_id, "Workspace 1".to_string(), cols, rows, pane_id, tab_id, surface_id, shell, &shell_args, waker.clone())?;

        let sidebar_width = settings.appearance.sidebar_width;
        let mut state = Self {
            workspaces: vec![ws],
            active_workspace: 0,
            next_ids,
            default_cols: cols,
            default_rows: rows,
            notifications: NotificationStore::with_coalesce_ms(settings.notification.coalesce_ms),
            notification_panel_open: false,
            settings,
            settings_open: false,
            settings_ui_state: SettingsUiState::new(),
            hook_manager: HookManager::new(),
            sidebar_width,
            waker,
            ws_rename: None,
            claude_parent_children: HashMap::new(),
            claude_child_parent: HashMap::new(),
            claude_closed_parents: HashSet::new(),
            claude_next_child_index: HashMap::new(),
            claude_idle_state: HashMap::new(),
            claude_needs_input_state: HashMap::new(),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            global_hook_manager: GlobalHookManager::new(),
            last_key_input: HashMap::new(),
            pane_context_menu: None,
            markdown_path_dialog: None,
        };
        state.send_fast_init(surface_id);
        Ok(state)
    }

    pub fn active_workspace(&self) -> &Workspace {
        let idx = self.active_workspace.min(self.workspaces.len().saturating_sub(1));
        &self.workspaces[idx]
    }

    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        let idx = self.active_workspace.min(self.workspaces.len().saturating_sub(1));
        &mut self.workspaces[idx]
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
        self.last_key_input.insert(surface_id, std::time::Instant::now());
    }

    /// Returns true if the surface received key input within the last 5 seconds.
    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    fn send_fast_init(&mut self, surface_id: u32) {
        crate::surface_meta::SurfaceMetaStore::ensure_created(surface_id);
        let scrollback_limit = self.settings.general.scrollback_lines;
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.set_scrollback_limit(scrollback_limit);
        }
        if let Some(cmd) = self.settings.general.fast_mode_init_command() {
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&cmd);
            }
        }
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
        let ws_id = self.next_ids.next_workspace();
        let pane_id = self.next_ids.next_pane();
        let tab_id = self.next_ids.next_tab();
        let surface_id = self.next_ids.next_surface();

        let name = format!("Workspace {}", self.workspaces.len() + 1);
        let shell = if self.settings.general.shell.is_empty() { None } else { Some(self.settings.general.shell.as_str()) };
        let shell_args_owned = self.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            self.default_cols,
            self.default_rows,
            pane_id,
            tab_id,
            surface_id,
            shell,
            &shell_args,
            self.waker.clone(),
        )?;
        self.workspaces.push(ws);
        self.active_workspace = self.workspaces.len() - 1;
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a new tab in the focused pane.
    pub fn add_tab(&mut self) -> anyhow::Result<()> {
        let tab_id = self.next_ids.next_tab();
        let surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;
        let shell = self.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.waker.clone();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_with_shell(tab_id, surface_id, cols, rows, shell_ref, &shell_args, waker)?;
        }
        self.send_fast_init(surface_id);
        Ok(())
    }

    /// Add a Markdown viewer tab in the focused pane.
    pub fn add_markdown_tab(&mut self, file_path: String) -> anyhow::Result<()> {
        let tab_id = self.next_ids.next_tab();
        let panel_id = self.next_ids.next_surface(); // reuse surface id counter
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_markdown_tab(tab_id, panel_id, file_path);
        }
        Ok(())
    }

    /// Add a file explorer tab in the focused pane.
    pub fn add_explorer_tab(&mut self, root_path: String) -> anyhow::Result<()> {
        let tab_id = self.next_ids.next_tab();
        let panel_id = self.next_ids.next_surface();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_explorer_tab(tab_id, panel_id, root_path);
        }
        Ok(())
    }

    /// Split the focused pane into two (new independent tab bar).
    pub fn split_pane(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_pane_id = self.next_ids.next_pane();
        let new_tab_id = self.next_ids.next_tab();
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;

        // Pre-create the new Pane (PTY allocation) BEFORE any structural mutation.
        // This way, if PTY creation fails, the layout is untouched.
        let shell = self.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.waker.clone())?;

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
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;
        let shell = self.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.waker.clone();
        if let Some(pane) = self.focused_pane_mut() {
            pane.split_active_surface_with_shell(direction, new_surface_id, cols, rows, shell_ref, &shell_args, waker)?;
        }
        self.send_fast_init(new_surface_id);
        Ok(())
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

    // ---- Claude parent-child management ----

    /// Get the next child index for a parent, incrementing the counter.
    pub fn next_child_index(&mut self, parent_id: u32) -> u32 {
        let idx = self.claude_next_child_index.entry(parent_id).or_insert(0);
        *idx += 1;
        *idx
    }

    /// Register a child entry under a parent surface.
    pub fn register_child(&mut self, parent_id: u32, entry: ClaudeChildEntry) {
        self.claude_child_parent.insert(entry.child_surface_id, parent_id);
        self.claude_parent_children.entry(parent_id).or_default().push(entry);
    }

    /// Unregister a child surface. Cleans up parent tracking if parent is closed and has no more children.
    pub fn unregister_child(&mut self, child_surface_id: u32) {
        self.claude_idle_state.remove(&child_surface_id);
        self.claude_needs_input_state.remove(&child_surface_id);
        if let Some(parent_id) = self.claude_child_parent.remove(&child_surface_id) {
            if let Some(children) = self.claude_parent_children.get_mut(&parent_id) {
                children.retain(|c| c.child_surface_id != child_surface_id);
                if children.is_empty() && self.claude_closed_parents.contains(&parent_id) {
                    self.claude_parent_children.remove(&parent_id);
                    self.claude_closed_parents.remove(&parent_id);
                    self.claude_next_child_index.remove(&parent_id);
                }
            }
        }
    }

    /// Mark a parent surface as closed. If it has no children, clean up immediately.
    pub fn mark_parent_closed(&mut self, parent_surface_id: u32) {
        self.claude_idle_state.remove(&parent_surface_id);
        self.claude_needs_input_state.remove(&parent_surface_id);
        if self.claude_parent_children.contains_key(&parent_surface_id) {
            let children_empty = self.claude_parent_children
                .get(&parent_surface_id)
                .map(|c| c.is_empty())
                .unwrap_or(true);
            if children_empty {
                self.claude_parent_children.remove(&parent_surface_id);
                self.claude_next_child_index.remove(&parent_surface_id);
            } else {
                self.claude_closed_parents.insert(parent_surface_id);
            }
        }
    }

    /// Get all children of a parent surface.
    pub fn children_of(&self, parent_id: u32) -> &[ClaudeChildEntry] {
        self.claude_parent_children.get(&parent_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get the parent of a child surface.
    pub fn parent_of(&self, child_id: u32) -> Option<u32> {
        self.claude_child_parent.get(&child_id).copied()
    }

    /// Set the Claude idle state for a surface. When becoming non-idle, also clears needs_input.
    pub fn set_claude_idle(&mut self, surface_id: u32, idle: bool) {
        self.claude_idle_state.insert(surface_id, idle);
        if !idle {
            self.claude_needs_input_state.remove(&surface_id);
        }
    }

    /// Set the Claude needs-input state for a surface.
    pub fn set_claude_needs_input(&mut self, surface_id: u32, needs_input: bool) {
        self.claude_needs_input_state.insert(surface_id, needs_input);
    }

    /// Get the Claude state string for a surface: "needs_input", "idle", or "active".
    pub fn claude_state_of(&self, surface_id: u32) -> &str {
        if self.claude_needs_input_state.get(&surface_id).copied().unwrap_or(false) {
            "needs_input"
        } else if self.claude_idle_state.get(&surface_id).copied().unwrap_or(false) {
            "idle"
        } else {
            "active"
        }
    }

    /// Split the focused pane and return the new surface ID.
    /// This is like `split_pane` but returns the new surface_id for callers that need it.
    pub fn split_pane_get_surface(&mut self, direction: SplitDirection) -> anyhow::Result<u32> {
        let new_pane_id = self.next_ids.next_pane();
        let new_tab_id = self.next_ids.next_tab();
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;

        let shell = self.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let shell_args_owned = self.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, &shell_args, self.waker.clone())?;

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
        for workspace in &self.workspaces {
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

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.active_workspace = index;
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
        for (i, workspace) in self.workspaces.iter_mut().enumerate() {
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
                let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
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

    /// Find a terminal by its surface ID across all workspaces (immutable).
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        for workspace in &self.workspaces {
            let layout = workspace.pane_layout();
            if let Some(t) = Self::find_terminal_in_layout(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by its surface ID across all workspaces (mutable).
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        for workspace in &mut self.workspaces {
            let layout = workspace.pane_layout_mut();
            if let Some(t) = Self::find_terminal_in_layout_mut(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    fn find_terminal_in_layout(layout: &crate::model::PaneNode, surface_id: u32) -> Option<&Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::find_terminal_in_layout(first, surface_id)
                    .or_else(|| Self::find_terminal_in_layout(second, surface_id))
            }
        }
    }

    fn find_terminal_in_layout_mut(layout: &mut crate::model::PaneNode, surface_id: u32) -> Option<&mut Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal_mut(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                if let Some(t) = Self::find_terminal_in_layout_mut(first, surface_id) {
                    return Some(t);
                }
                Self::find_terminal_in_layout_mut(second, surface_id)
            }
        }
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
        self.default_cols = cols;
        self.default_rows = rows;
    }

    /// Resize all terminals in the active workspace to match a given terminal rect.
    pub fn resize_all(&mut self, terminal_rect: Rect, cell_width: f32, cell_height: f32) {
        let ws = self.active_workspace_mut();
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
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
        for workspace in &mut self.workspaces {
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
            for workspace in &mut self.workspaces {
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
            for workspace in &mut self.workspaces {
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
        let tab_bar_h = if tab_count > 1 { 24.0 } else { 0.0 };
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
        let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
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

        let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
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
        self.surface_next_message_id += 1;
        let id = self.surface_next_message_id;
        let msg = SurfaceMessage { id, from_surface_id: from, content };
        self.surface_messages.entry(to).or_default().push(msg);
        id
    }

    /// Read (and optionally consume) messages queued for a surface.
    /// If `from` is Some, only return messages from that sender.
    /// If `peek` is false, the returned messages are removed from the queue.
    pub fn read_messages(&mut self, surface_id: u32, from: Option<u32>, peek: bool) -> Vec<SurfaceMessage> {
        let queue = match self.surface_messages.get_mut(&surface_id) {
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
        self.surface_messages.get(&surface_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Clear all messages queued for a surface.
    pub fn clear_messages(&mut self, surface_id: u32) {
        self.surface_messages.remove(&surface_id);
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
        for ws in &mut state.workspaces {
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

        let ws_count_before = state.workspaces.len();
        let tab_count_before = state.active_workspace().pane_layout().all_pane_ids().len();

        let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
        state.focus_pane(pane_ids[0]);

        assert_eq!(state.workspaces.len(), ws_count_before);
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
        assert_eq!(state.workspaces.len(), 1);
        state.add_workspace().unwrap();
        assert_eq!(state.workspaces.len(), 2);
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
