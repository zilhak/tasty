//! 폴링으로 surface·워크스페이스·탭 포커스와 pane 간 탭 이동을 감지한다.

use super::{PendingHostEvent, RequestContext};
use crate::core::CoreState;

impl RequestContext {
    pub fn detect_focus_change(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events = Vec::new();
        let current = self.focused_surface_id(engine);
        if current == self.last_focused_surface_id {
            return events;
        }
        let prev = self.last_focused_surface_id;
        self.last_focused_surface_id = current;
        if let Some(surface_id) = current {
            events.push(PendingHostEvent::SurfaceFocused {
                surface_id,
                prev_surface_id: prev,
            });
        }
        events
    }

    pub fn detect_workspace_activation(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events = Vec::new();
        let current = engine
            .workspace_at(self.active_workspace_index(engine))
            .map(|w| w.id);
        if current == self.last_active_workspace_id {
            return events;
        }
        let prev = self.last_active_workspace_id;
        self.last_active_workspace_id = current;
        if let Some(workspace_id) = current {
            events.push(PendingHostEvent::WorkspaceActivated {
                workspace_id,
                prev_workspace_id: prev,
            });
        }
        events
    }

    pub fn detect_tab_focus_change(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events = Vec::new();
        let current = self.focused_pane(engine).and_then(|pane| {
            pane.tabs
                .get(self.navigation.tab_index(pane))
                .map(|tab| (pane.id, tab.id))
        });
        if current == self.last_focused_tab {
            return events;
        }
        let prev_tab_id = self.last_focused_tab.map(|(_, tab_id)| tab_id);
        self.last_focused_tab = current;
        if let Some((pane_id, tab_id)) = current {
            events.push(PendingHostEvent::TabFocused {
                tab_id,
                pane_id,
                prev_tab_id,
            });
        }
        events
    }
}
