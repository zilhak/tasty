//! 폴링으로 surface·워크스페이스·탭 포커스와 pane 간 탭 이동을 감지한다.

use super::{AppState, PendingHostEvent};
use crate::core::CoreState;

impl AppState {
    pub fn detect_focus_change(&mut self, engine: &CoreState) {
        let current = self.focused_surface_id(engine);
        if current == self.last_focused_surface_id {
            return;
        }
        let prev = self.last_focused_surface_id;
        self.last_focused_surface_id = current;
        if let Some(surface_id) = current {
            self.enqueue_host_event(PendingHostEvent::SurfaceFocused {
                surface_id,
                prev_surface_id: prev,
            });
        }
    }

    pub fn detect_workspace_activation(&mut self, engine: &CoreState) {
        let current = engine.workspaces.get(self.active_workspace).map(|w| w.id);
        if current == self.last_active_workspace_id {
            return;
        }
        let prev = self.last_active_workspace_id;
        self.last_active_workspace_id = current;
        if let Some(workspace_id) = current {
            self.enqueue_host_event(PendingHostEvent::WorkspaceActivated {
                workspace_id,
                prev_workspace_id: prev,
            });
        }
    }

    pub fn detect_tab_focus_change(&mut self, engine: &CoreState) {
        let current = self
            .focused_pane(engine)
            .and_then(|pane| pane.tabs.get(pane.active_tab).map(|tab| (pane.id, tab.id)));
        if current == self.last_focused_tab {
            return;
        }
        let prev_tab_id = self.last_focused_tab.map(|(_, tab_id)| tab_id);
        self.last_focused_tab = current;
        if let Some((pane_id, tab_id)) = current {
            self.enqueue_host_event(PendingHostEvent::TabFocused {
                tab_id,
                pane_id,
                prev_tab_id,
            });
        }
    }

    /// pane 간 탭 이동을 감지한다. 생성·닫기는 해당 처리 경로가 이벤트를 기록한다.
    /// 최초 호출은 기준 사본만 만든다.
    pub fn detect_tab_lifecycle(&mut self, engine: &CoreState) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, (u32, u32, String)> = HashMap::new();
        for ws in &engine.workspaces {
            let workspace_id = ws.id;
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        let kind = tab
                            .focused_surface_id()
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
                return;
            }
        };

        for (tab_id, (pane_id, _, _)) in &current {
            if let Some((prev_pane, _, _)) = prev.get(tab_id)
                && prev_pane != pane_id
            {
                self.pending_host_events.push(PendingHostEvent::TabMoved {
                    tab_id: *tab_id,
                    from_pane: *prev_pane,
                    to_pane: *pane_id,
                });
            }
        }

        self.last_tab_locations = Some(current);
    }
}
