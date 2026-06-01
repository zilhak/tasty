//! Polling 기반 lifecycle 이벤트 감지: focus 변화, workspace activation,
//! tab/pane/workspace/surface 의 created/closed/moved 감지.
//!
//! 호스트 main loop tick 마다 `AppState` 의 스냅샷을 직전 tick 과 비교해 변경이
//! 있으면 `pending_host_events` 큐에 `PendingHostEvent` 를 enqueue 한다.

use super::{AppState, PendingHostEvent};
use crate::engine_state::CoreState;

impl AppState {
    /// 현재 focused surface id를 마지막 기록과 비교해 달라졌다면 `SurfaceFocused`
    /// 이벤트를 enqueue하고 기록을 갱신한다. focus 전환 경로(키/마우스/IPC/탭/워크
    /// 스페이스)가 많아 각각 hook하는 대신 main loop tick에서 polling으로 처리한다.
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

    /// 현재 활성 워크스페이스 ID를 마지막 기록과 비교해 달라졌다면 `WorkspaceActivated`
    /// 이벤트를 enqueue한다. workspace 활성화 경로(사이드바 클릭, 단축키, IPC 등)가
    /// 여럿이라 focused와 동일하게 polling으로 처리.
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

    /// focused pane의 active tab을 마지막 기록과 비교해 달라졌다면 `TabFocused`
    /// 이벤트를 enqueue. tab 전환 경로(클릭, next/prev/goto 단축키, close 후 인접
    /// 탭으로 shift, pane 전환에 의한 focused tab 변화 등)가 여럿이라 polling 채택.
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

    /// Tab 의 cross-pane 이동 (`tab.moved`) 만 polling 으로 감지한다.
    /// `tab.created` / `tab.closed` 는 D.3.C.B.10.1 이후 cascade 시점에 직접
    /// enqueue 하므로 여기서 다루지 않는다 (중복 발화 방지). cross-pane move 는
    /// 별 DomainIntent 가 없어 polling 으로 잡는다.
    /// 첫 호출(스냅샷이 `None`)에서는 베이스라인만 기록한다.
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
            if let Some((prev_pane, _, _)) = prev.get(tab_id) {
                if prev_pane != pane_id {
                    self.pending_host_events.push(PendingHostEvent::TabMoved {
                        tab_id: *tab_id,
                        from_pane: *prev_pane,
                        to_pane: *pane_id,
                    });
                }
            }
        }

        self.last_tab_locations = Some(current);
    }

    /// Pane lifecycle baseline 만 갱신 (D.3.C.B.10.2 이후). 실제 `pane.created`/
    /// `pane.closed` 발화는 cascade 시점에 `cascade_pane_split` /
    /// `cascade_pane_closed` 에서 enqueue 한다. baseline 은 cascade 가 직접
    /// `lifecycle_baseline_insert_pane` / `_remove_pane` 헬퍼로 동기화하며, 본
    /// 함수는 첫 호출 시 baseline 만 잡고 이후엔 polling 으로 push 하지 않는다.
    /// B.10.6 에서 호출처 제거 후 함수 자체를 제거할 예정.
    pub fn detect_pane_lifecycle(&mut self, engine: &CoreState) {
        use std::collections::HashMap;

        if self.last_pane_locations.is_some() {
            return;
        }
        let mut current: HashMap<u32, u32> = HashMap::new();
        for ws in &engine.workspaces {
            for pane_id in ws.pane_layout().all_pane_ids() {
                current.insert(pane_id, ws.id);
            }
        }
        self.last_pane_locations = Some(current);
    }

    /// Workspace lifecycle baseline 만 갱신 (D.3.C.B.10.4 이후). 실제
    /// `workspace.created`/`workspace.closed` 발화는 cascade 시점에
    /// `cascade_workspace_created` / `cascade_surface_closed` (Workspace level)
    /// 에서 enqueue 한다. baseline 은 cascade 가 `lifecycle_baseline_insert_workspace`
    /// / `_remove_workspace` 헬퍼로 동기화. B.10.6 에서 함수 제거 예정.
    pub fn detect_workspace_lifecycle(&mut self, engine: &CoreState, _window_id: u64) {
        use std::collections::HashMap;

        if self.last_workspace_snapshot.is_some() {
            return;
        }
        let mut current: HashMap<u32, String> = HashMap::new();
        for ws in &engine.workspaces {
            current.insert(ws.id, ws.name.clone());
        }
        self.last_workspace_snapshot = Some(current);
    }

    /// Surface lifecycle baseline 만 갱신 (D.3.C.B.10.3 이후). 실제
    /// `surface.created` 발화는 cascade 시점에 `cascade_surface_created`
    /// (TabCreated / PaneSplit / SurfaceSplit / WorkspaceCreated) 가 enqueue 한다.
    /// baseline 은 cascade 가 `lifecycle_baseline_insert_surface` 헬퍼로 동기화하며,
    /// 본 함수는 첫 호출 시 baseline 만 잡는다. B.10.6 에서 호출처 제거 후 함수
    /// 자체 제거 예정.
    pub fn detect_surface_lifecycle(&mut self, engine: &CoreState) {
        use std::collections::HashMap;

        if self.last_surface_locations.is_some() {
            return;
        }
        let mut current: HashMap<u32, (u32, u32, u32, &'static str)> = HashMap::new();
        for ws in &engine.workspaces {
            let workspace_id = ws.id;
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        let tab_id = tab.id;
                        if let Some(layout) = tab.layout_if_initialized() {
                            for sid in layout.all_surface_ids() {
                                let kind = layout
                                    .find_surface(sid)
                                    .map(|s| s.kind())
                                    .unwrap_or("unknown");
                                current.insert(sid, (tab_id, pane_id, workspace_id, kind));
                            }
                        }
                    }
                }
            }
        }
        self.last_surface_locations = Some(current);
    }
}
