//! Polling 기반 lifecycle 이벤트 감지: focus 변화, workspace activation,
//! tab/pane/workspace/surface 의 created/closed/moved 감지.
//!
//! 호스트 main loop tick 마다 `AppState` 의 스냅샷을 직전 tick 과 비교해 변경이
//! 있으면 `pending_host_events` 큐에 `PendingHostEvent` 를 enqueue 한다.

use super::{AppState, PendingHostEvent};
use crate::engine_state::EngineState;

impl AppState {
    /// 현재 focused surface id를 마지막 기록과 비교해 달라졌다면 `SurfaceFocused`
    /// 이벤트를 enqueue하고 기록을 갱신한다. focus 전환 경로(키/마우스/IPC/탭/워크
    /// 스페이스)가 많아 각각 hook하는 대신 main loop tick에서 polling으로 처리한다.
    pub fn detect_focus_change(&mut self, engine: &EngineState) {
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
    pub fn detect_workspace_activation(&mut self, engine: &EngineState) {
        let current = engine
            .workspaces
            .get(self.active_workspace)
            .map(|w| w.id);
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
    pub fn detect_tab_focus_change(&mut self, engine: &EngineState) {
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

    /// 전체 워크스페이스를 순회하며 현재 (tab_id → pane_id, workspace_id, kind) 매핑을
    /// 마지막 스냅샷과 비교해 `TabCreated`/`TabClosed`/`TabMoved` 이벤트를 enqueue한다.
    /// 첫 호출(스냅샷이 `None`)에서는 이벤트를 발화하지 않고 베이스라인만 기록한다 —
    /// 앱 시작 시 이미 로드된 탭들이 잘못 `tab.created`로 보고되지 않도록 하기 위함.
    pub fn detect_tab_lifecycle(&mut self, engine: &EngineState) {
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

        for (tab_id, (pane_id, workspace_id, kind)) in &current {
            match prev.get(tab_id) {
                None => {
                    self.pending_host_events.push(PendingHostEvent::TabCreated {
                        tab_id: *tab_id,
                        pane_id: *pane_id,
                        workspace_id: *workspace_id,
                        kind: kind.clone(),
                    });
                }
                Some((prev_pane, _, _)) if prev_pane != pane_id => {
                    self.pending_host_events.push(PendingHostEvent::TabMoved {
                        tab_id: *tab_id,
                        from_pane: *prev_pane,
                        to_pane: *pane_id,
                    });
                }
                _ => {}
            }
        }
        for (tab_id, (pane_id, _, _)) in &prev {
            if !current.contains_key(tab_id) {
                self.pending_host_events.push(PendingHostEvent::TabClosed {
                    tab_id: *tab_id,
                    pane_id: *pane_id,
                });
            }
        }

        self.last_tab_locations = Some(current);
    }

    /// Pane 생성/종료를 polling으로 감지. `last_tab_locations`와 동일하게 첫 호출
    /// 에서는 베이스라인만 기록한다.
    pub fn detect_pane_lifecycle(&mut self, engine: &EngineState) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, u32> = HashMap::new();
        for ws in &engine.workspaces {
            for pane_id in ws.pane_layout().all_pane_ids() {
                current.insert(pane_id, ws.id);
            }
        }

        let prev = match self.last_pane_locations.take() {
            Some(p) => p,
            None => {
                self.last_pane_locations = Some(current);
                return;
            }
        };

        for (pane_id, workspace_id) in &current {
            if !prev.contains_key(pane_id) {
                self.pending_host_events
                    .push(PendingHostEvent::PaneCreated {
                        pane_id: *pane_id,
                        workspace_id: *workspace_id,
                    });
            }
        }
        for pane_id in prev.keys() {
            if !current.contains_key(pane_id) {
                self.pending_host_events
                    .push(PendingHostEvent::PaneClosed { pane_id: *pane_id });
            }
        }

        self.last_pane_locations = Some(current);
    }

    /// Workspace 생성/종료를 polling으로 감지. `window_id`는 caller가 전달하며
    /// (이 `AppState`가 속한 main window의 winit::WindowId를 u64로 변환), 신규
    /// workspace가 발견되면 `WorkspaceCreated`에 채워 넣는다.
    pub fn detect_workspace_lifecycle(&mut self, engine: &EngineState, window_id: u64) {
        use std::collections::HashMap;

        let mut current: HashMap<u32, String> = HashMap::new();
        for ws in &engine.workspaces {
            current.insert(ws.id, ws.name.clone());
        }

        let prev = match self.last_workspace_snapshot.take() {
            Some(p) => p,
            None => {
                self.last_workspace_snapshot = Some(current);
                return;
            }
        };

        for (workspace_id, name) in &current {
            if !prev.contains_key(workspace_id) {
                self.pending_host_events
                    .push(PendingHostEvent::WorkspaceCreated {
                        workspace_id: *workspace_id,
                        window_id,
                        name: name.clone(),
                    });
            }
        }
        for workspace_id in prev.keys() {
            if !current.contains_key(workspace_id) {
                self.pending_host_events
                    .push(PendingHostEvent::WorkspaceClosed {
                        workspace_id: *workspace_id,
                    });
            }
        }

        self.last_workspace_snapshot = Some(current);
    }

    /// Surface 생성을 polling으로 감지. 신규 surface_id가 발견되면 `SurfaceCreated`를
    /// `created_by_plugin: None` (User)로 enqueue한다. Plugin이 spawn한 surface는
    /// 향후 plugin spawn IPC 핸들러에서 별도로 `Agent { source_plugin }` 컨텍스트를
    /// 채워 직접 enqueue하는 경로를 둘 예정. surface.closed는 별도 큐가 처리하므로
    /// 여기서는 생성만 감지.
    pub fn detect_surface_lifecycle(&mut self, engine: &EngineState) {
        use std::collections::HashMap;

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

        let prev = match self.last_surface_locations.take() {
            Some(p) => p,
            None => {
                self.last_surface_locations = Some(current);
                return;
            }
        };

        for (surface_id, (tab_id, pane_id, workspace_id, kind)) in &current {
            if !prev.contains_key(surface_id) {
                self.pending_host_events
                    .push(PendingHostEvent::SurfaceCreated {
                        surface_id: *surface_id,
                        kind,
                        tab_id: *tab_id,
                        pane_id: *pane_id,
                        workspace_id: *workspace_id,
                        created_by_plugin: None,
                    });
            }
        }

        self.last_surface_locations = Some(current);
    }
}
