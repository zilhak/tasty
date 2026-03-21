use crate::model::{PaneId, Rect, SplitDirection, Workspace};
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::settings_ui::SettingsUiState;
use crate::terminal::{Terminal, TerminalEvent};

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
}

impl AppState {
    /// Creates initial state with one workspace, one pane, one tab, one terminal.
    pub fn new(cols: usize, rows: usize) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let ws = Workspace::new(0, "Workspace 1".to_string(), cols, rows, 0, 0, 0)?;
        Ok(Self {
            workspaces: vec![ws],
            active_workspace: 0,
            next_ids: IdGenerator::new(),
            default_cols: cols,
            default_rows: rows,
            notifications: NotificationStore::new(),
            notification_panel_open: false,
            settings,
            settings_open: false,
            settings_ui_state: SettingsUiState::new(),
        })
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    /// Get the focused pane in the active workspace.
    pub fn focused_pane(&self) -> &crate::model::Pane {
        let ws = self.active_workspace();
        ws.pane_layout
            .find_pane(ws.focused_pane)
            .expect("focused pane not found")
    }

    /// Get the focused pane (mutable) in the active workspace.
    pub fn focused_pane_mut(&mut self) -> &mut crate::model::Pane {
        let ws = self.active_workspace_mut();
        let focused_id = ws.focused_pane;
        ws.pane_layout
            .find_pane_mut(focused_id)
            .expect("focused pane not found")
    }

    /// Get the ultimately focused terminal.
    pub fn focused_terminal(&self) -> &Terminal {
        self.focused_pane().active_terminal()
    }

    /// Get the ultimately focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> &mut Terminal {
        self.focused_pane_mut().active_terminal_mut()
    }

    /// Add a new workspace with one pane, one tab, one terminal.
    pub fn add_workspace(&mut self) -> anyhow::Result<()> {
        let ws_id = self.next_ids.next_workspace();
        let pane_id = self.next_ids.next_pane();
        let tab_id = self.next_ids.next_tab();
        let surface_id = self.next_ids.next_surface();

        let name = format!("Workspace {}", ws_id + 1);
        let ws = Workspace::new(
            ws_id,
            name,
            self.default_cols,
            self.default_rows,
            pane_id,
            tab_id,
            surface_id,
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
        self.focused_pane_mut()
            .add_tab(tab_id, surface_id, cols, rows)?;
        Ok(())
    }

    /// Split the focused pane into two (new independent tab bar).
    pub fn split_pane(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_pane_id = self.next_ids.next_pane();
        let new_tab_id = self.next_ids.next_tab();
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;

        let ws = self.active_workspace_mut();
        let target_pane_id = ws.focused_pane;
        ws.pane_layout.split_pane(
            target_pane_id,
            direction,
            new_pane_id,
            new_tab_id,
            new_surface_id,
            cols,
            rows,
        )?;
        // Focus the new pane
        ws.focused_pane = new_pane_id;
        Ok(())
    }

    /// Split within the current tab (SurfaceGroup). Appears as one tab.
    pub fn split_surface(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let new_surface_id = self.next_ids.next_surface();
        let cols = self.default_cols;
        let rows = self.default_rows;
        self.focused_pane_mut()
            .active_panel_mut()
            .split_surface(direction, new_surface_id, cols, rows)?;
        Ok(())
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.active_workspace = index;
        }
    }

    /// Next tab in the focused pane.
    pub fn next_tab_in_pane(&mut self) {
        self.focused_pane_mut().next_tab();
    }

    /// Previous tab in the focused pane.
    pub fn prev_tab_in_pane(&mut self) {
        self.focused_pane_mut().prev_tab();
    }

    /// Move focus to the next pane.
    pub fn move_focus_next_pane(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout.next_pane_id(ws.focused_pane);
    }

    /// Move focus to the previous pane.
    pub fn move_focus_prev_pane(&mut self) {
        let ws = self.active_workspace_mut();
        ws.focused_pane = ws.pane_layout.prev_pane_id(ws.focused_pane);
    }

    /// Process all terminals in the active workspace. Returns true if any changed.
    pub fn process_all(&mut self) -> bool {
        self.active_workspace_mut().pane_layout.process_all()
    }

    /// Compute all render regions for the active workspace.
    /// Returns: for each pane, the pane rect and the terminal regions within it.
    pub fn render_regions(
        &self,
        terminal_rect: Rect,
    ) -> Vec<(PaneId, Rect, Vec<(u32, &Terminal, Rect)>)> {
        let ws = self.active_workspace();
        let pane_rects = ws.pane_layout.compute_rects(terminal_rect);

        let mut result = Vec::new();
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout.find_pane(pane_id) {
                // Reserve space for tab bar at top of each pane
                let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                let regions = pane.active_panel().render_regions(content_rect);
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
        let pane_rects = ws.pane_layout.compute_rects(terminal_rect);
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout.find_pane_mut(pane_id) {
                let tab_bar_h = if pane.tabs.len() > 1 { 24.0 } else { 0.0 };
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                pane.active_panel_mut()
                    .resize_all(content_rect, cell_width, cell_height);
            }
        }
    }

    /// Get the focused pane ID.
    pub fn focused_pane_id(&self) -> PaneId {
        self.active_workspace().focused_pane
    }

    /// Collect events from all terminals in the active workspace.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for terminal in self.active_workspace_mut().pane_layout.all_terminals_mut() {
            all_events.extend(terminal.take_events());
        }
        all_events
    }
}
