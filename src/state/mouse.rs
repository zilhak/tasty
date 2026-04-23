use crate::model::{DividerInfo, PhysicalPx, Rect, SplitDirection};

use super::AppState;

impl AppState {
    /// Determine the cursor icon for the winit (non-egui) area at the given position.
    /// Checks dividers first, then asks the surface. Returns None if not over any winit area.
    pub fn winit_cursor_icon_at(
        &self,
        x: f32,
        y: f32,
        terminal_rect: Rect,
        divider_threshold: f32,
    ) -> Option<egui::CursorIcon> {
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }

        // 1. Divider check
        let divider = self
            .find_pane_divider_at(x, y, terminal_rect, divider_threshold)
            .or_else(|| self.find_surface_divider_at(x, y, terminal_rect, divider_threshold));
        if let Some(info) = divider {
            return Some(match info.direction {
                SplitDirection::Vertical => egui::CursorIcon::ResizeHorizontal,
                SplitDirection::Horizontal => egui::CursorIcon::ResizeVertical,
            });
        }

        // 2. Surface check — ask the surface for its cursor
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(terminal_rect) {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    let local_x = x - r.rect.x.value();
                    let local_y = y - r.rect.y.value();
                    return r.surface.cursor_icon_at(local_x, local_y);
                }
            }
        }

        None
    }

    /// Find a pane-level divider at the given position.
    pub fn find_pane_divider_at(
        &self,
        x: f32,
        y: f32,
        terminal_rect: Rect,
        threshold: f32,
    ) -> Option<DividerInfo> {
        let ws = self.active_workspace();
        ws.pane_layout()
            .find_divider_at(x, y, terminal_rect, threshold)
    }

    /// Find a surface-level divider at the given position (within the focused pane's panel).
    pub fn find_surface_divider_at(
        &self,
        x: f32,
        y: f32,
        terminal_rect: Rect,
        threshold: f32,
    ) -> Option<DividerInfo> {
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
            height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
        };

        let tab = pane.tabs.get(pane.active_tab)?;
        tab.layout().find_divider_at(x, y, content_rect, threshold)
    }

    /// Update a pane-level split ratio based on a divider drag.
    pub fn update_pane_divider(
        &mut self,
        divider: &DividerInfo,
        x: f32,
        y: f32,
        terminal_rect: Rect,
    ) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => {
                (PhysicalPx(x) - divider.split_rect.x).value() / divider.split_rect.width.value()
            }
            SplitDirection::Horizontal => {
                (PhysicalPx(y) - divider.split_rect.y).value() / divider.split_rect.height.value()
            }
        };
        let ws = self.active_workspace_mut();
        let updated = ws
            .pane_layout_mut()
            .update_ratio_for_rect(divider.split_rect, new_ratio, terminal_rect);
        if updated {
            self.engine.mark_layout_dirty();
        }
        updated
    }

    /// Update a surface-level split ratio based on a divider drag.
    pub fn update_surface_divider(
        &mut self,
        divider: &DividerInfo,
        x: f32,
        y: f32,
        terminal_rect: Rect,
    ) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => {
                (PhysicalPx(x) - divider.split_rect.x).value() / divider.split_rect.width.value()
            }
            SplitDirection::Horizontal => {
                (PhysicalPx(y) - divider.split_rect.y).value() / divider.split_rect.height.value()
            }
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
            height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
        };

        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => return false,
        };

        let updated = tab
            .layout_mut()
            .update_ratio_for_rect(divider.split_rect, new_ratio, content_rect);
        if updated {
            self.engine.mark_layout_dirty();
        }
        updated
    }
}
