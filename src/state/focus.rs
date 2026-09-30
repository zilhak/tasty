use crate::core::CoreState;
use crate::model::PhysicalPx;

use super::AppState;

impl AppState {
    pub fn move_pane_focus_forward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace(engine);
        if let Some(id) = self.navigation.pane_id(ws) {
            self.navigation
                .select_pane(ws, ws.pane_layout().next_pane_id(id));
        }
    }

    pub fn move_pane_focus_backward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace(engine);
        if let Some(id) = self.navigation.pane_id(ws) {
            self.navigation
                .select_pane(ws, ws.pane_layout().prev_pane_id(id));
        }
    }

    pub fn move_surface_focus_forward(&mut self, engine: &mut CoreState) {
        self.move_surface_focus(engine, false);
    }

    pub fn move_surface_focus_backward(&mut self, engine: &mut CoreState) {
        self.move_surface_focus(engine, true);
    }

    fn move_surface_focus(&mut self, engine: &CoreState, backward: bool) {
        let Some(pane) = self.focused_pane(engine) else {
            return;
        };
        let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
            return;
        };
        let ids = tab.all_surface_ids();
        if ids.is_empty() {
            return;
        }
        let current = self.navigation.surface_id(tab);
        let position = ids.iter().position(|id| Some(*id) == current).unwrap_or(0);
        let step = if backward { ids.len() - 1 } else { 1 };
        self.navigation
            .select_surface(tab, ids[(position + step) % ids.len()]);
    }

    pub fn focus_pane_at_position(
        &mut self,
        engine: &mut CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let ws = self.active_workspace(engine);
        for (pane_id, rect) in ws.pane_layout().compute_rects(terminal_rect, scale_factor) {
            if rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                return self.navigation.select_pane(ws, pane_id);
            }
        }
        false
    }

    pub fn focus_surface_at_position(
        &mut self,
        engine: &mut CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let ws = self.active_workspace(engine);
        let Some(pane_id) = self.navigation.pane_id(ws) else {
            return false;
        };
        let Some((_, rect)) = ws
            .pane_layout()
            .compute_rects(terminal_rect, scale_factor)
            .into_iter()
            .find(|(id, _)| *id == pane_id)
        else {
            return false;
        };
        let content = crate::model::PhysicalRect {
            x: rect.x,
            y: rect.y + self.tab_bar_height,
            width: rect.width,
            height: (rect.height - self.tab_bar_height).max(PhysicalPx(1.0)),
        };
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            return false;
        };
        let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
            return false;
        };
        tab.layout()
            .find_surface_at(x, y, content)
            .is_some_and(|id| self.navigation.select_surface(tab, id))
    }

    /// Native input may focus a surface only in a currently displayed tab.
    pub fn focus_surface_by_id(&mut self, engine: &mut CoreState, surface_id: u32) -> bool {
        let ws = self.active_workspace(engine);
        for pane_id in ws.pane_layout().all_pane_ids() {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
                continue;
            };
            if tab.contains_surface(surface_id) {
                let pane_changed = self.navigation.select_pane(ws, pane_id);
                let surface_changed = self.navigation.select_surface(tab, surface_id);
                return pane_changed || surface_changed;
            }
        }
        false
    }
}
