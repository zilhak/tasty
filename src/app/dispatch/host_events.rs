//! 호스트 자동 발화 큐(`PendingHostEvent`) → Event Bus 1.0 broadcast.
//!
//! 본 메서드는 295 라인 — 30+ 종류의 PendingHostEvent variant 를 각각 wire payload 로
//! 변환·발화한다. Level 3 에서 더 잘게 쪼갤 후보.

use crate::app::App;

impl App {
    /// 호스트 자동 발화 큐(`PendingHostEvent`)를 모든 AppState에서 drain해 Event Bus
    /// 1.0 wire payload로 변환·발화한다. focus처럼 발화 시점을 일일이 hook하기 번거로운
    /// 이벤트는 먼저 `detect_focus_change()`로 변화 검사 후 queue에 push된다.
    pub(crate) fn dispatch_pending_host_events(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::LifecycleReason;
        use tasty_plugin_protocol::events::payloads::{
            HookFired, NotificationCreated, PaneClosed, PaneCreated, PaneSplit, ProcessExited,
            SplitDirection, SurfaceCreated, SurfaceCreatedBy, SurfaceFocused, SurfaceResized,
            SurfaceTitleChanged, TabClosed, TabCreated, TabFocused, TabMoved, TabRenamed,
            WorkspaceActivated, WorkspaceClosed, WorkspaceCreated, WorkspaceRenamed,
        };

        let mut drained: Vec<crate::state::PendingHostEvent> = Vec::new();
        for (win_id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                main.state.detect_focus_change();
                main.state.detect_workspace_activation();
                main.state.detect_tab_focus_change();
                main.state.detect_tab_lifecycle();
                main.state.detect_pane_lifecycle();
                main.state.detect_workspace_lifecycle(u64::from(*win_id));
                main.state.detect_surface_lifecycle();
                drained.extend(main.state.take_pending_host_events());
            }
        }
        for s in &mut self.parked_states {
            s.detect_focus_change();
            s.detect_workspace_activation();
            s.detect_tab_focus_change();
            s.detect_tab_lifecycle();
            s.detect_pane_lifecycle();
            // parked AppState은 더 이상 window에 붙어있지 않으므로 workspace.created
            // 발화에 의미 있는 window_id를 채울 수 없다. workspace.closed만 의도하는
            // 경우라도 polling은 새 workspace를 detect할 수 없게 베이스라인부터
            // 비교가 필요하다. window 분리 직전의 detect에서 이미 베이스라인이
            // 형성됐다고 가정하고 동일 호출 — window_id는 0 (sentinel).
            s.detect_workspace_lifecycle(0);
            s.detect_surface_lifecycle();
            drained.extend(s.take_pending_host_events());
        }
        if drained.is_empty() {
            return;
        }
        let lua = self.lua_engine.as_ref();
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ev in drained {
            match ev {
                crate::state::PendingHostEvent::SurfaceFocused {
                    surface_id,
                    prev_surface_id,
                } => {
                    let payload = SurfaceFocused {
                        surface_id,
                        prev_surface_id,
                    };
                    mgr.emit_host_event("surface.focused", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::SurfaceResized {
                    surface_id,
                    width_px,
                    height_px,
                } => {
                    let payload = SurfaceResized {
                        surface_id,
                        width_px,
                        height_px,
                    };
                    mgr.emit_host_event_throttled(
                        "surface.resized",
                        surface_id as u64,
                        &payload,
                        EventScope::Surface,
                    );
                }
                crate::state::PendingHostEvent::SurfaceTitleChanged { surface_id, title } => {
                    let payload = SurfaceTitleChanged { surface_id, title };
                    mgr.emit_host_event("surface.title_changed", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::SurfaceCreated {
                    surface_id,
                    kind,
                    tab_id,
                    pane_id,
                    workspace_id,
                    created_by_plugin,
                } => {
                    let created_by = match created_by_plugin {
                        Some(pid) => SurfaceCreatedBy::Agent { source_plugin: pid },
                        None => SurfaceCreatedBy::User,
                    };
                    let payload = SurfaceCreated {
                        surface_id,
                        kind: kind.to_string(),
                        tab_id,
                        pane_id,
                        workspace_id,
                        created_by,
                    };
                    mgr.emit_host_event("surface.created", &payload, EventScope::Surface);
                    crate::hooks::lua::fire(lua, "surface.create.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceActivated {
                    workspace_id,
                    prev_workspace_id,
                } => {
                    let payload = WorkspaceActivated {
                        workspace_id,
                        prev_workspace_id,
                    };
                    mgr.emit_host_event("workspace.activated", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::WorkspaceRenamed {
                    workspace_id,
                    name,
                    subtitle,
                    description,
                    user_direct,
                } => {
                    let payload = WorkspaceRenamed {
                        workspace_id,
                        name,
                        subtitle,
                        description,
                    };
                    mgr.emit_host_event("workspace.renamed", &payload, EventScope::System);
                    // 사용자 직접 변경(GUI rename dialog)만 Lua hook 발화 — IPC 경유는 제외.
                    if user_direct {
                        crate::hooks::lua::fire(lua, "workspace.change.post", &payload);
                    }
                }
                crate::state::PendingHostEvent::TabFocused {
                    tab_id,
                    pane_id,
                    prev_tab_id,
                } => {
                    let payload = TabFocused {
                        tab_id,
                        pane_id,
                        prev_tab_id,
                    };
                    mgr.emit_host_event("tab.focused", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::TabRenamed {
                    tab_id,
                    title,
                    user_direct,
                } => {
                    let payload = TabRenamed { tab_id, title };
                    mgr.emit_host_event("tab.renamed", &payload, EventScope::System);
                    if user_direct {
                        crate::hooks::lua::fire(lua, "tab.change.post", &payload);
                    }
                }
                crate::state::PendingHostEvent::ProcessExited { surface_id } => {
                    let payload = ProcessExited {
                        surface_id,
                        exit_code: None,
                    };
                    mgr.emit_host_event("process.exited", &payload, EventScope::Surface);
                }
                crate::state::PendingHostEvent::NotificationCreated {
                    id,
                    title,
                    body,
                    source,
                } => {
                    let payload = NotificationCreated {
                        id: id.to_string(),
                        title,
                        body,
                        source,
                    };
                    mgr.emit_host_event("notification.created", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::TabCreated {
                    tab_id,
                    pane_id,
                    workspace_id,
                    kind,
                } => {
                    let payload = TabCreated {
                        tab_id,
                        pane_id,
                        workspace_id,
                        kind,
                    };
                    mgr.emit_host_event("tab.created", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "tab.create.post", &payload);
                }
                crate::state::PendingHostEvent::TabClosed { tab_id, pane_id } => {
                    let payload = TabClosed {
                        tab_id,
                        pane_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("tab.closed", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "tab.delete.post", &payload);
                }
                crate::state::PendingHostEvent::TabMoved {
                    tab_id,
                    from_pane,
                    to_pane,
                } => {
                    let payload = TabMoved {
                        tab_id,
                        from_pane,
                        to_pane,
                    };
                    mgr.emit_host_event("tab.moved", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::PaneCreated {
                    pane_id,
                    workspace_id,
                } => {
                    let payload = PaneCreated {
                        pane_id,
                        parent_pane_group: None,
                        workspace_id,
                    };
                    mgr.emit_host_event("pane.created", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "pane.create.post", &payload);
                }
                crate::state::PendingHostEvent::PaneClosed { pane_id } => {
                    let payload = PaneClosed {
                        pane_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("pane.closed", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "pane.delete.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceCreated {
                    workspace_id,
                    window_id,
                    name,
                } => {
                    let payload = WorkspaceCreated {
                        workspace_id,
                        window_id,
                        name,
                    };
                    mgr.emit_host_event("workspace.created", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "workspace.create.post", &payload);
                }
                crate::state::PendingHostEvent::WorkspaceClosed { workspace_id } => {
                    let payload = WorkspaceClosed {
                        workspace_id,
                        reason: LifecycleReason::User,
                    };
                    mgr.emit_host_event("workspace.closed", &payload, EventScope::System);
                    crate::hooks::lua::fire(lua, "workspace.delete.post", &payload);
                }
                crate::state::PendingHostEvent::HookFired {
                    hook_id,
                    event_kind,
                    surface_id,
                } => {
                    let scope = if surface_id != 0 {
                        EventScope::Surface
                    } else {
                        EventScope::System
                    };
                    let payload = HookFired {
                        hook_id: hook_id.to_string(),
                        event_kind,
                        surface_id: if surface_id != 0 { Some(surface_id) } else { None },
                        payload: serde_json::Value::Null,
                    };
                    mgr.emit_host_event("hook.fired", &payload, scope);
                }
                crate::state::PendingHostEvent::PaneSplit {
                    original_pane,
                    new_pane,
                    direction,
                } => {
                    let direction = match direction {
                        crate::model::SplitDirection::Horizontal => SplitDirection::Horizontal,
                        crate::model::SplitDirection::Vertical => SplitDirection::Vertical,
                    };
                    let payload = PaneSplit {
                        original_pane,
                        new_pane,
                        direction,
                    };
                    mgr.emit_host_event("pane.split", &payload, EventScope::System);
                }
                crate::state::PendingHostEvent::Raw { key, payload } => {
                    mgr.emit_host_event(&key, &payload, EventScope::System);
                }
            }
        }
    }
}
