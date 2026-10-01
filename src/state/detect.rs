//! 폴링으로 surface·워크스페이스·탭 포커스와 pane 간 탭 이동을 감지한다.

use super::{PendingHostEvent, RequestContext};
use crate::core::CoreState;

impl RequestContext {
    pub fn detect_focus_change(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events=Vec::new();
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
        let mut events=Vec::new();
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
        let mut events=Vec::new();
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

    /// pane 간 탭 이동을 감지한다. 생성·닫기는 해당 처리 경로가 이벤트를 기록한다.
    /// 최초 호출은 기준 사본만 만든다.
    pub fn detect_tab_lifecycle(&mut self, engine: &CoreState) -> Vec<PendingHostEvent> {
        let mut events=Vec::new();
        use std::collections::HashMap;

        let mut current: HashMap<u32, (u32, u32, String)> = HashMap::new();
        for ws in &engine.workspaces() {
            let workspace_id = ws.id;
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        let kind = self
                            .navigation
                            .surface_id(tab)
                            .and_then(|sid| engine.find_surface_by_id(sid))
                            .map(|s| s.kind().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        current.insert(tab.id, (pane_id, workspace_id, kind));
                    }
                }
            }
        }

        let prev = match self.last_tab_locations.take() {
            Some(p) => p,
            None => {
                self.last_tab_locations = Some(current);
                return events;
            }
        };

        for (tab_id, (pane_id, _, _)) in &current {
            if let Some((prev_pane, _, _)) = prev.get(tab_id)
                && prev_pane != pane_id
            {
                events.push(PendingHostEvent::TabMoved {
                    tab_id: *tab_id,
                    from_pane: *prev_pane,
                    to_pane: *pane_id,
                });
            }
        }

        self.last_tab_locations = Some(current);
        events
    }
}
