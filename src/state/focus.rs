use crate::core::CoreState;
use crate::model::PhysicalPx;

use super::AppState;

impl AppState {
    pub fn move_pane_focus_forward(&mut self, engine: &mut CoreState) {
        let ws = self.active_workspace_mut(engine);
        ws.focused_pane = ws.pane_layout().next_pane_id(ws.focused_pane);
    }

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
        scale_factor: f32,
    ) -> bool {
        let ws = self.active_workspace(engine);
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);
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
        scale_factor: f32,
    ) -> bool {
        let ws = self.active_workspace(engine);
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);

        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return false,
        };

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

    /// 활성 워크스페이스에서 ID로 포커스를 옮기고 변경 여부를 반환한다.
    /// 좌표를 받지 못하는 네이티브 WebView 입력에도 사용하며, 각 pane의 활성 탭만 찾는다.
    pub fn focus_surface_by_id(&mut self, engine: &mut CoreState, surface_id: u32) -> bool {
        let ws = self.active_workspace(engine);
        let mut target_pane = None;
        for pane_id in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id)
                && let Some(tab) = pane.tabs.get(pane.active_tab)
                && let Some(layout) = tab.layout_if_initialized()
                && layout.find_surface(surface_id).is_some()
            {
                target_pane = Some(pane_id);
                break;
            }
        }
        let Some(pane_id) = target_pane else {
            return false;
        };
        let ws = self.active_workspace_mut(engine);
        let mut changed = false;
        if ws.focused_pane != pane_id {
            ws.focused_pane = pane_id;
            changed = true;
        }
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
            && let Some(tab) = pane.active_tab_mut()
            && tab.focused_surface != surface_id
        {
            tab.focused_surface = surface_id;
            changed = true;
        }
        changed
    }
}
