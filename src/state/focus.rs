use crate::model::FocusDirection;

use super::AppState;

impl AppState {
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

    /// Focus the pane at the given physical pixel position within the terminal rect.
    /// Returns true if focus changed.
    pub fn focus_pane_at_position(&mut self, x: f32, y: f32, terminal_rect: crate::model::Rect) -> bool {
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
    pub fn focus_surface_at_position(&mut self, x: f32, y: f32, terminal_rect: crate::model::Rect) -> bool {
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
        let content_rect = crate::model::Rect {
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
}
