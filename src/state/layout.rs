use crate::model::{PaneId, Rect};
use tasty_terminal::Terminal;

use super::AppState;

impl AppState {
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

    /// Get the physical pixel rect of a specific terminal cell within a surface.
    pub fn surface_cell_rect(
        &self,
        terminal_rect: Rect,
        surface_id: u32,
        col: usize,
        row: usize,
        cell_w: f32,
        cell_h: f32,
    ) -> Option<Rect> {
        let regions = self.render_regions(terminal_rect);
        for (_pane_id, _pane_rect, terminal_regions) in &regions {
            for (sid, _term, rect) in terminal_regions {
                if *sid == surface_id {
                    return Some(Rect {
                        x: rect.x + col as f32 * cell_w,
                        y: rect.y + row as f32 * cell_h,
                        width: cell_w.max(1.0),
                        height: cell_h.max(1.0),
                    });
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
}
