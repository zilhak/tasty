use crate::model::{DividerInfo, Rect, SplitDirection};

use super::AppState;

impl AppState {
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
}
