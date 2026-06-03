use crate::core::CoreState;
use crate::model::PhysicalPx;

use super::AppState;

impl AppState {
    /// Move focus to the next pane only (skip surface group logic).
    pub fn move_pane_focus_forward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

    /// Move focus to the previous pane only (skip surface group logic).
    pub fn move_pane_focus_backward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = ws.pane_layout().prev_pane_id(ws.focused_pane);
    }

    /// Move focus to the next surface within the current tab's split.
    /// Does nothing if not in a multi-surface tab.
    pub fn move_surface_focus_forward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
            && let Some(tab) = pane.active_tab_mut()
        {
            tab.move_focus_forward();
        }
    }

    /// Move focus to the previous surface within the current tab's split.
    /// Does nothing if not in a multi-surface tab.
    pub fn move_surface_focus_backward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
            && let Some(tab) = pane.active_tab_mut()
        {
            tab.move_focus_backward();
        }
    }

    /// Focus the pane at the given physical pixel position within the terminal rect.
    /// Returns true if focus changed.
    pub fn focus_pane_at_position(
        &mut self,
        engine: &mut CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
    ) -> bool {
        let ws = self.active_workspace(engine);
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
        for (pane_id, rect) in pane_rects {
            if rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                let old = self.active_workspace(engine).focused_pane;
                if old != pane_id {
                    self.active_workspace_mut(engine).focused_pane = pane_id;
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// Focus the surface (within a split tab) at the given physical pixel position.
    /// This should be called after focus_pane_at_position to also focus within the pane's panel.
    /// Returns true if focus changed.
    pub fn focus_surface_at_position(
        &mut self,
        engine: &mut CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
    ) -> bool {
        let ws = self.active_workspace(engine);
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

        // Find the focused pane's rect
        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return false,
        };

        // Account for tab bar height
        let ws = self.active_workspace(engine);
        let _tab_count = ws
            .pane_layout()
            .find_pane(focused_id)
            .map(|p| p.tabs.len())
            .unwrap_or(0);
        let tab_bar_h = self.tab_bar_height;
        let content_rect = crate::model::PhysicalRect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
        };

        let ws = self.active_workspace_mut(engine);
        let pane = match ws.pane_layout_mut().find_pane_mut(focused_id) {
            Some(p) => p,
            None => return false,
        };

        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => return false,
        };

        if let Some(surface_id) = tab.layout().find_surface_at(x, y, content_rect)
            && tab.focused_surface != surface_id
        {
            tab.focused_surface = surface_id;
            return true;
        }
        false
    }
}
