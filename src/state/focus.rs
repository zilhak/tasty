use crate::core::CoreState;
use crate::model::{FocusDirection, PhysicalPx};

use super::AppState;

impl AppState {
    /// Move focus forward: within the active tab's split first, then between panes.
    pub fn move_focus_forward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;

        // Try to move within a split tab first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                if tab.all_surface_ids().len() > 1 {
                    tab.move_focus_forward();
                    return;
                }
            }
        }

        // Not in a multi-surface tab, move between panes
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

    /// Move focus backward: within the active tab's split first, then between panes.
    pub fn move_focus_backward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;

        // Try to move within a split tab first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                if tab.all_surface_ids().len() > 1 {
                    tab.move_focus_backward();
                    return;
                }
            }
        }

        // Not in a multi-surface tab, move between panes
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = ws.pane_layout().prev_pane_id(ws.focused_pane);
    }

    /// Move focus in a spatial direction (left/right/up/down).
    /// First tries to move within a split tab, then moves between panes.
    pub fn move_focus_direction(&mut self, engine: &mut CoreState, direction: FocusDirection) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;

        // Try to move within a split tab first
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                if let Some(new_surface_id) = tab.directional_focus(direction) {
                    tab.focused_surface = new_surface_id;
                    return;
                }
            }
        }

        // Try to move between panes
        let ws = self.active_workspace_mut(engine);
        if let Some(target_pane_id) = ws
            .pane_layout()
            .directional_focus(ws.focused_pane, direction)
        {
            ws.focused_pane = target_pane_id;
        }
    }

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
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                tab.move_focus_forward();
            }
        }
    }

    /// Move focus to the previous surface within the current tab's split.
    /// Does nothing if not in a multi-surface tab.
    pub fn move_surface_focus_backward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        let pane_id = ws.focused_pane;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                tab.move_focus_backward();
            }
        }
    }

    /// Set the focused pane in the active workspace to the given pane_id.
    /// Returns true if the pane exists.
    pub fn focus_pane(&mut self, engine: &mut CoreState, pane_id: u32) -> bool {
        let ws = self.active_workspace_mut(engine);
        if ws.pane_layout().find_pane(pane_id).is_some() {
            ws.focused_pane = pane_id;
            true
        } else {
            false
        }
    }

    /// Find which pane contains the surface, focus that pane, and if it's in a split tab,
    /// focus that surface. Searches all workspaces, not just the active one.
    /// Returns true if found.
    pub fn focus_surface(&mut self, engine: &mut CoreState, surface_id: u32) -> bool {
        // Search all workspaces for the surface.
        let mut found_ws_idx = None;
        let mut found_pane_id = None;
        for (ws_idx, ws) in engine.workspaces.iter().enumerate() {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    if pane.contains_surface(surface_id) {
                        found_ws_idx = Some(ws_idx);
                        found_pane_id = Some(pid);
                        break;
                    }
                }
            }
            if found_pane_id.is_some() {
                break;
            }
        }
        let (ws_idx, pane_id) = match (found_ws_idx, found_pane_id) {
            (Some(wi), Some(pi)) => (wi, pi),
            _ => return false,
        };
        // Switch to the workspace containing the surface.
        self.active_workspace = ws_idx;
        // Focus the pane.
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = pane_id;
        // If the active tab contains this surface, focus it within the tab.
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                if tab.contains_surface(surface_id) {
                    tab.focused_surface = surface_id;
                }
            }
        }
        true
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

        if let Some(surface_id) = tab.layout().find_surface_at(x, y, content_rect) {
            if tab.focused_surface != surface_id {
                tab.focused_surface = surface_id;
                return true;
            }
        }
        false
    }
}
