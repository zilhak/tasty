use crate::hooks::HookManager;
use crate::model::{DividerInfo, PaneId, Rect, SplitDirection, Workspace};
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::settings_ui::SettingsUiState;
use crate::terminal::{Terminal, TerminalEvent, Waker};

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
        let ws = Workspace::new_with_shell(ws_id, "Workspace 1".to_string(), cols, rows, pane_id, tab_id, surface_id, shell, waker.clone())?;
        let sidebar_width = settings.appearance.sidebar_width;
        Ok(Self {
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
        })
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

        let name = format!("Workspace {}", ws_id + 1);
        let shell = if self.settings.general.shell.is_empty() { None } else { Some(self.settings.general.shell.as_str()) };
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            self.default_cols,
            self.default_rows,
            pane_id,
            tab_id,
            surface_id,
            shell,
            self.waker.clone(),
        )?;
        self.workspaces.push(ws);
        self.active_workspace = self.workspaces.len() - 1;
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
        let waker = self.waker.clone();
        if let Some(pane) = self.focused_pane_mut() {
            pane.add_tab_with_shell(tab_id, surface_id, cols, rows, shell_ref, waker)?;
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
        let new_pane =
            crate::model::Pane::new_with_shell(new_pane_id, new_tab_id, new_surface_id, cols, rows, shell_ref, self.waker.clone())?;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        // Focus the new pane
        ws.focused_pane = new_pane_id;
        Ok(())
    }

    /// Split within the current tab (SurfaceGroup). Appears as one tab.
    pub fn split_surface(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;
        let shell = self.settings.general.shell.clone();
        let shell_ref = if shell.is_empty() { None } else { Some(shell.as_str()) };
        let waker = self.waker.clone();
        if let Some(pane) = self.focused_pane_mut() {
            pane.split_active_surface_with_shell(direction, new_surface_id, cols, rows, shell_ref, waker)?;
        }
        Ok(())
    }

    /// Close the active tab in the focused pane. Returns true if a tab was closed.
    pub fn close_active_tab(&mut self) -> bool {
        if let Some(pane) = self.focused_pane_mut() {
            pane.close_active_tab()
        } else {
            false
        }
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self) -> bool {
        let ws = self.active_workspace_mut();
        let target_id = ws.focused_pane;
        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
        }
        removed
    }

    /// Close the focused surface within a SurfaceGroup. Returns true if a surface was removed.
    pub fn close_active_surface(&mut self) -> bool {
        if let Some(pane) = self.focused_pane_mut() {
            if let Some(panel) = pane.active_panel_mut() {
                match panel {
                    crate::model::Panel::SurfaceGroup(group) => {
                        let target = group.focused_surface;
                        group.close_surface(target)
                    }
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        }
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

    /// Move focus to the next pane.
    pub fn move_focus_next_pane(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

    /// Move focus to the previous pane.
    pub fn move_focus_prev_pane(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout().prev_pane_id(ws.focused_pane);
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
            Self::collect_events_from_pane_node(workspace.pane_layout_mut(), &mut all_events);
        }
        all_events
    }

    fn collect_events_from_pane_node(node: &mut crate::model::PaneNode, out: &mut Vec<TerminalEvent>) {
        match node {
            crate::model::PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    Self::collect_events_from_panel(tab.panel_mut(), out);
                }
            }
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::collect_events_from_pane_node(first, out);
                Self::collect_events_from_pane_node(second, out);
            }
        }
    }

    fn collect_events_from_panel(panel: &mut crate::model::Panel, out: &mut Vec<TerminalEvent>) {
        match panel {
            crate::model::Panel::Terminal(node) => {
                let sid = node.id;
                let mut events = node.terminal.take_events();
                for event in &mut events {
                    event.surface_id = sid;
                }
                out.extend(events);
            }
            crate::model::Panel::SurfaceGroup(group) => {
                Self::collect_events_from_surface_layout(group.layout_mut(), out);
            }
        }
    }

    fn collect_events_from_surface_layout(layout: &mut crate::model::SurfaceGroupLayout, out: &mut Vec<TerminalEvent>) {
        match layout {
            crate::model::SurfaceGroupLayout::Single(node) => {
                let sid = node.id;
                let mut events = node.terminal.take_events();
                for event in &mut events {
                    event.surface_id = sid;
                }
                out.extend(events);
            }
            crate::model::SurfaceGroupLayout::Split { first, second, .. } => {
                Self::collect_events_from_surface_layout(first, out);
                Self::collect_events_from_surface_layout(second, out);
            }
        }
    }

    /// Set a read mark on the focused terminal (or a specific surface).
    pub fn set_mark(&mut self, surface_id: Option<u32>) {
        if let Some(_sid) = surface_id {
            // Find terminal by surface ID - walk the tree
            self.set_mark_on_surface(_sid);
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.set_mark();
        }
    }

    /// Read since mark on the focused terminal (or a specific surface).
    pub fn read_since_mark(&mut self, surface_id: Option<u32>, strip_ansi: bool) -> String {
        if let Some(sid) = surface_id {
            self.read_since_mark_on_surface(sid, strip_ansi)
                .unwrap_or_default()
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.read_since_mark(strip_ansi)
        } else {
            String::new()
        }
    }

    /// Set mark on a specific surface by ID.
    fn set_mark_on_surface(&mut self, surface_id: u32) {
        for workspace in &mut self.workspaces {
            if Self::set_mark_in_pane_node(workspace.pane_layout_mut(), surface_id) {
                return;
            }
        }
    }

    fn set_mark_in_pane_node(
        node: &mut crate::model::PaneNode,
        surface_id: u32,
    ) -> bool {
        match node {
            crate::model::PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    if Self::set_mark_in_panel(tab.panel_mut(), surface_id) {
                        return true;
                    }
                }
                false
            }
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::set_mark_in_pane_node(first, surface_id)
                    || Self::set_mark_in_pane_node(second, surface_id)
            }
        }
    }

    fn set_mark_in_panel(
        panel: &mut crate::model::Panel,
        surface_id: u32,
    ) -> bool {
        match panel {
            crate::model::Panel::Terminal(node) => {
                if node.id == surface_id {
                    node.terminal.set_mark();
                    return true;
                }
                false
            }
            crate::model::Panel::SurfaceGroup(group) => {
                Self::set_mark_in_surface_layout(group.layout_mut(), surface_id)
            }
        }
    }

    fn set_mark_in_surface_layout(
        layout: &mut crate::model::SurfaceGroupLayout,
        surface_id: u32,
    ) -> bool {
        match layout {
            crate::model::SurfaceGroupLayout::Single(node) => {
                if node.id == surface_id {
                    node.terminal.set_mark();
                    return true;
                }
                false
            }
            crate::model::SurfaceGroupLayout::Split { first, second, .. } => {
                Self::set_mark_in_surface_layout(first, surface_id)
                    || Self::set_mark_in_surface_layout(second, surface_id)
            }
        }
    }

    /// Read since mark on a specific surface by ID.
    fn read_since_mark_on_surface(
        &mut self,
        surface_id: u32,
        strip_ansi: bool,
    ) -> Option<String> {
        for workspace in &mut self.workspaces {
            if let Some(text) =
                Self::read_mark_in_pane_node(workspace.pane_layout_mut(), surface_id, strip_ansi)
            {
                return Some(text);
            }
        }
        None
    }

    fn read_mark_in_pane_node(
        node: &mut crate::model::PaneNode,
        surface_id: u32,
        strip_ansi: bool,
    ) -> Option<String> {
        match node {
            crate::model::PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    if let Some(text) =
                        Self::read_mark_in_panel(tab.panel_mut(), surface_id, strip_ansi)
                    {
                        return Some(text);
                    }
                }
                None
            }
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::read_mark_in_pane_node(first, surface_id, strip_ansi)
                    .or_else(|| Self::read_mark_in_pane_node(second, surface_id, strip_ansi))
            }
        }
    }

    fn read_mark_in_panel(
        panel: &mut crate::model::Panel,
        surface_id: u32,
        strip_ansi: bool,
    ) -> Option<String> {
        match panel {
            crate::model::Panel::Terminal(node) => {
                if node.id == surface_id {
                    return Some(node.terminal.read_since_mark(strip_ansi));
                }
                None
            }
            crate::model::Panel::SurfaceGroup(group) => {
                Self::read_mark_in_surface_layout(group.layout_mut(), surface_id, strip_ansi)
            }
        }
    }

    fn read_mark_in_surface_layout(
        layout: &mut crate::model::SurfaceGroupLayout,
        surface_id: u32,
        strip_ansi: bool,
    ) -> Option<String> {
        match layout {
            crate::model::SurfaceGroupLayout::Single(node) => {
                if node.id == surface_id {
                    return Some(node.terminal.read_since_mark(strip_ansi));
                }
                None
            }
            crate::model::SurfaceGroupLayout::Split { first, second, .. } => {
                Self::read_mark_in_surface_layout(first, surface_id, strip_ansi).or_else(|| {
                    Self::read_mark_in_surface_layout(second, surface_id, strip_ansi)
                })
            }
        }
    }

    /// Get the next surface ID (for creating new terminals).
    pub fn next_surface_id(&mut self) -> u32 {
        self.next_ids.next_surface()
    }

    /// Get the next workspace ID.
    pub fn next_workspace_id(&mut self) -> u32 {
        self.next_ids.next_workspace()
    }

    /// Get the next pane ID.
    pub fn next_pane_id(&mut self) -> u32 {
        self.next_ids.next_pane()
    }

    /// Get the next tab ID.
    pub fn next_tab_id(&mut self) -> u32 {
        self.next_ids.next_tab()
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
}
