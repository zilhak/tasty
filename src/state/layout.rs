use crate::core::CoreState;
use crate::model::{PaneId, PhysicalPx, PhysicalRect, SurfaceRegion};

use super::AppState;

impl AppState {
    /// Process all terminals in ALL workspaces to drain PTY channels.
    /// Returns true if the active workspace had any changes (for redraw).
    pub fn process_all(&mut self, engine: &mut CoreState) -> bool {
        let active_idx = self.active_workspace;
        let active_ids: std::collections::HashSet<u32> = engine
            .workspaces
            .get(active_idx)
            .map(|ws| ws.all_surface_ids().into_iter().collect())
            .unwrap_or_default();
        let mut active_changed = false;
        for (sid, t) in engine.terminals.iter_mut() {
            if t.process() && active_ids.contains(&sid) {
                active_changed = true;
            }
        }
        active_changed
    }

    /// Compute all surface regions for the active workspace.
    /// Returns: for each pane, the pane rect and all surface regions within it.
    pub fn surface_regions<'a>(
        &self,
        engine: &'a CoreState,
        terminal_rect: PhysicalRect,
    ) -> Vec<(PaneId, PhysicalRect, Vec<SurfaceRegion<'a>>)> {
        let ws = self.active_workspace(engine);
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        let mut result = Vec::new();
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                let tab_bar_h = self.tab_bar_height;
                let content_rect = PhysicalRect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
                };
                let regions = match pane.tabs.get(pane.active_tab) {
                    Some(tab) => tab.surface_regions(content_rect),
                    None => Vec::new(),
                };
                result.push((pane_id, pane_rect, regions));
            }
        }
        result
    }

    /// Get the actual content rect for the focused surface (accounting for tab bar).
    /// Returns None if no surface is focused.
    pub fn focused_surface_rect(
        &self,
        engine: &CoreState,
        terminal_rect: PhysicalRect,
    ) -> Option<PhysicalRect> {
        let surface_id = self.focused_surface_id(engine)?;
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(engine, terminal_rect) {
            for r in regions {
                if r.id == surface_id {
                    return Some(r.rect);
                }
            }
        }
        None
    }

    /// Get the physical pixel rect of a specific terminal cell within a surface.
    #[allow(clippy::too_many_arguments)] // reason: cell geometry lookup 컨텍스트
    pub fn surface_cell_rect(
        &self,
        engine: &CoreState,
        terminal_rect: PhysicalRect,
        surface_id: u32,
        col: usize,
        row: usize,
        cell_w: f32,
        cell_h: f32,
    ) -> Option<PhysicalRect> {
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(engine, terminal_rect) {
            for r in regions {
                if r.id == surface_id {
                    return Some(PhysicalRect {
                        x: r.rect.x + PhysicalPx(col as f32 * cell_w),
                        y: r.rect.y + PhysicalPx(row as f32 * cell_h),
                        width: PhysicalPx(cell_w.max(1.0)),
                        height: PhysicalPx(cell_h.max(1.0)),
                    });
                }
            }
        }
        None
    }

    /// Get the rect of a specific surface by id.
    pub fn surface_rect_by_id(
        &self,
        engine: &CoreState,
        surface_id: u32,
        terminal_rect: PhysicalRect,
    ) -> Option<PhysicalRect> {
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(engine, terminal_rect) {
            for r in regions {
                if r.id == surface_id {
                    return Some(r.rect);
                }
            }
        }
        None
    }

    /// Find the surface ID at the given physical pixel position.
    pub fn surface_id_at_position(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
    ) -> Option<u32> {
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(engine, terminal_rect) {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    return Some(r.id);
                }
            }
        }
        None
    }

    /// Resize all terminals in all workspaces and all tabs to match a given terminal rect.
    pub fn resize_all(
        &mut self,
        engine: &mut CoreState,
        terminal_rect: PhysicalRect,
        cell_width: f32,
        cell_height: f32,
    ) {
        let tab_bar_h = self.tab_bar_height;
        for ws in &mut engine.workspaces {
            let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
            for (pane_id, pane_rect) in pane_rects {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    let content_rect = PhysicalRect {
                        x: pane_rect.x,
                        y: pane_rect.y + tab_bar_h,
                        width: pane_rect.width,
                        height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
                    };
                    for tab in &mut pane.tabs {
                        tab.resize_all(content_rect, cell_width, cell_height);
                    }
                }
            }
        }

        // Note: PTY resize is deferred (pending_pty_resize in Terminal).
        // Callers should call flush_all_pty_resizes() when resize events settle,
        // NOT on every frame during continuous drag.
    }
}
