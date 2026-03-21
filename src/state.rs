use crate::model::{Pane, Rect, SplitDirection, Workspace};
use crate::terminal::Terminal;

pub struct AppState {
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    next_workspace_id: u32,
    next_pane_id: u32,
    next_surface_id: u32,
    cols: usize,
    rows: usize,
}

impl AppState {
    /// Creates initial state with one workspace, one pane, one surface.
    pub fn new(cols: usize, rows: usize) -> anyhow::Result<Self> {
        let ws = Workspace::new(0, "Workspace 1".to_string(), cols, rows, 0, 0)?;
        Ok(Self {
            workspaces: vec![ws],
            active_workspace: 0,
            next_workspace_id: 1,
            next_pane_id: 1,
            next_surface_id: 1,
            cols,
            rows,
        })
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    pub fn active_pane(&self) -> &Pane {
        self.active_workspace().active_pane()
    }

    pub fn active_pane_mut(&mut self) -> &mut Pane {
        self.active_workspace_mut().active_pane_mut()
    }

    pub fn focused_terminal(&self) -> &Terminal {
        self.active_pane().root.focused_terminal()
    }

    pub fn focused_terminal_mut(&mut self) -> &mut Terminal {
        self.active_pane_mut().root.focused_terminal_mut()
    }

    /// Add a new workspace with one default pane.
    pub fn add_workspace(&mut self) -> anyhow::Result<()> {
        let ws_id = self.next_workspace_id;
        self.next_workspace_id += 1;
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let surface_id = self.next_surface_id;
        self.next_surface_id += 1;

        let name = format!("Workspace {}", ws_id + 1);
        let ws = Workspace::new(ws_id, name, self.cols, self.rows, surface_id, pane_id)?;
        self.workspaces.push(ws);
        self.active_workspace = self.workspaces.len() - 1;
        Ok(())
    }

    /// Add a new pane to the active workspace.
    pub fn add_pane(&mut self) -> anyhow::Result<()> {
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let surface_id = self.next_surface_id;
        self.next_surface_id += 1;

        let pane = Pane::new(pane_id, "Shell".to_string(), self.cols, self.rows, surface_id)?;
        self.active_workspace_mut().add_pane(pane);
        Ok(())
    }

    /// Split the focused surface in the active pane.
    pub fn split_focused(&mut self, direction: SplitDirection) -> anyhow::Result<()> {
        let surface_id = self.next_surface_id;
        self.next_surface_id += 1;
        let cols = self.cols;
        let rows = self.rows;

        self.active_pane_mut()
            .root
            .split(direction, surface_id, cols, rows)?;
        Ok(())
    }

    /// Process all terminals in the active workspace. Returns true if any changed.
    pub fn process_all(&mut self) -> bool {
        let ws = &mut self.workspaces[self.active_workspace];
        let mut changed = false;
        for pane in &mut ws.panes {
            if pane.root.process_all() {
                changed = true;
            }
        }
        changed
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.active_workspace = index;
        }
    }

    /// Switch to the next pane in the active workspace.
    pub fn next_pane(&mut self) {
        let ws = &mut self.workspaces[self.active_workspace];
        if ws.panes.len() > 1 {
            ws.active_pane = (ws.active_pane + 1) % ws.panes.len();
        }
    }

    /// Move focus forward within the active pane's split tree.
    pub fn move_focus_forward(&mut self) {
        self.active_pane_mut().root.move_focus_forward();
    }

    /// Move focus backward within the active pane's split tree.
    pub fn move_focus_backward(&mut self) {
        self.active_pane_mut().root.move_focus_backward();
    }

    /// Update stored grid dimensions and resize all terminals in the active pane.
    pub fn update_grid_size(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
    }

    /// Resize all terminals in the active pane to fit the given terminal rect.
    pub fn resize_active_pane(&mut self, terminal_rect: Rect, cell_width: f32, cell_height: f32) {
        self.active_pane_mut()
            .root
            .resize_all(terminal_rect, cell_width, cell_height);
    }

    /// Get render regions for the active pane.
    pub fn render_regions(&self, terminal_rect: Rect) -> Vec<(u32, &Terminal, Rect)> {
        self.active_pane().root.render_regions(terminal_rect)
    }
}
