use crate::core::CoreState;
use crate::model::PhysicalPx;

use super::RequestContext;

impl RequestContext {
    pub fn move_pane_focus_forward(&mut self, engine: &CoreState) {
        let ws = self.active_workspace(engine);
        if let Some(id) = self.navigation.pane_id(ws) {
            self.navigation
                .select_pane(ws, ws.pane_layout().next_pane_id(id));
        }
    }

    pub fn move_pane_focus_backward(&mut self, engine: &CoreState) {
        let ws = self.active_workspace(engine);
        if let Some(id) = self.navigation.pane_id(ws) {
            self.navigation
                .select_pane(ws, ws.pane_layout().prev_pane_id(id));
        }
    }

    pub fn move_surface_focus_forward(&mut self, engine: &CoreState) {
        self.move_surface_focus(engine, false);
    }

    pub fn move_surface_focus_backward(&mut self, engine: &CoreState) {
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
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let ws = self.active_workspace(engine);
        #[cfg(feature = "gui")]
        let pane_rects = self.pane_rects(engine, ws, terminal_rect, scale_factor);
        #[cfg(not(feature = "gui"))]
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);
        for (pane_id, rect) in pane_rects {
            if rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                return self.navigation.select_pane(ws, pane_id);
            }
        }
        false
    }

    pub fn focus_surface_at_position(
        &mut self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let Some(id) = self.surface_at_position(engine, x, y, terminal_rect, scale_factor) else {
            return false;
        };
        let ws = self.active_workspace(engine);
        let Some(pane_id) = self.navigation.pane_id(ws) else {
            return false;
        };
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            return false;
        };
        let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
            return false;
        };
        self.navigation.select_surface(tab, id)
    }

    /// 포커스된 pane 의 보이는 탭에서 좌표 아래 surface. 포커스는 바꾸지 않는다.
    pub fn surface_at_position(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: crate::model::PhysicalRect,
        scale_factor: f32,
    ) -> Option<u32> {
        let ws = self.active_workspace(engine);
        let pane_id = self.navigation.pane_id(ws)?;
        #[cfg(feature = "gui")]
        let pane_rects = self.pane_rects(engine, ws, terminal_rect, scale_factor);
        #[cfg(not(feature = "gui"))]
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);
        let (_, rect) = pane_rects.into_iter().find(|(id, _)| *id == pane_id)?;
        let content = crate::model::PhysicalRect {
            x: rect.x,
            y: rect.y + self.tab_bar_height,
            width: rect.width,
            height: (rect.height - self.tab_bar_height).max(PhysicalPx(1.0)),
        };
        let pane = ws.pane_layout().find_pane(pane_id)?;
        let tab = pane.tabs.get(self.navigation.tab_index(pane))?;
        #[cfg(feature = "gui")]
        let surface = self
            .tab_surface_regions(engine, tab, content, scale_factor)
            .into_iter()
            .find(|region| region.rect.contains(PhysicalPx(x), PhysicalPx(y)))
            .map(|region| region.id);
        #[cfg(not(feature = "gui"))]
        let surface = tab.layout().find_surface_at(x, y, content);
        surface
    }

    /// Native input may focus a surface only in a currently displayed tab.
    pub fn focus_surface_by_id(&mut self, engine: &CoreState, surface_id: u32) -> bool {
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
