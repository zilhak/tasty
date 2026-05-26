//! 호스트 자동 발화 큐(`PendingHostEvent`)를 Event Bus 1.0 wire payload 로 변환·발화.
//!
//! 변환·발화 로직은 도메인별 sub-module 의 free fn 으로 분리. 본 모듈은 drain +
//! match dispatch.

mod misc;
mod pane;
mod surface;
mod tab;
mod workspace;

use crate::app::App;
use crate::state::PendingHostEvent;

impl App {
    pub(crate) fn dispatch_pending_host_events(&mut self) {
        let mut drained: Vec<PendingHostEvent> = Vec::new();
        for (win_id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                let engine = &mut main.engine_state;
                main.state.detect_focus_change(engine);
                main.state.detect_workspace_activation(engine);
                main.state.detect_tab_focus_change(engine);
                main.state.detect_tab_lifecycle(engine);
                main.state.detect_pane_lifecycle(engine);
                main.state
                    .detect_workspace_lifecycle(engine, u64::from(*win_id));
                main.state.detect_surface_lifecycle(engine);
                drained.extend(main.state.take_pending_host_events());
            }
        }
        // parked (AppState, EngineState) 쌍은 자기 짝의 engine 으로 detect.
        for (s, engine) in self.parked_states.iter_mut() {
            s.detect_focus_change(engine);
            s.detect_workspace_activation(engine);
            s.detect_tab_focus_change(engine);
            s.detect_tab_lifecycle(engine);
            s.detect_pane_lifecycle(engine);
            s.detect_workspace_lifecycle(engine, 0);
            s.detect_surface_lifecycle(engine);
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
                PendingHostEvent::SurfaceFocused {
                    surface_id,
                    prev_surface_id,
                } => surface::emit_focused(mgr, surface_id, prev_surface_id),
                PendingHostEvent::SurfaceResized {
                    surface_id,
                    width_px,
                    height_px,
                } => surface::emit_resized(mgr, surface_id, width_px, height_px),
                PendingHostEvent::SurfaceTitleChanged { surface_id, title } => {
                    surface::emit_title_changed(mgr, surface_id, title)
                }
                PendingHostEvent::SurfaceCreated {
                    surface_id,
                    kind,
                    tab_id,
                    pane_id,
                    workspace_id,
                    created_by_plugin,
                } => surface::emit_created(
                    mgr,
                    lua,
                    surface_id,
                    kind,
                    tab_id,
                    pane_id,
                    workspace_id,
                    created_by_plugin,
                ),
                PendingHostEvent::WorkspaceActivated {
                    workspace_id,
                    prev_workspace_id,
                } => workspace::emit_activated(mgr, workspace_id, prev_workspace_id),
                PendingHostEvent::WorkspaceRenamed {
                    workspace_id,
                    name,
                    subtitle,
                    description,
                    user_direct,
                } => workspace::emit_renamed(
                    mgr,
                    lua,
                    workspace_id,
                    name,
                    subtitle,
                    description,
                    user_direct,
                ),
                PendingHostEvent::WorkspaceCreated {
                    workspace_id,
                    window_id,
                    name,
                } => workspace::emit_created(mgr, lua, workspace_id, window_id, name),
                PendingHostEvent::WorkspaceClosed { workspace_id } => {
                    workspace::emit_closed(mgr, lua, workspace_id)
                }
                PendingHostEvent::TabFocused {
                    tab_id,
                    pane_id,
                    prev_tab_id,
                } => tab::emit_focused(mgr, tab_id, pane_id, prev_tab_id),
                PendingHostEvent::TabRenamed {
                    tab_id,
                    title,
                    user_direct,
                } => tab::emit_renamed(mgr, lua, tab_id, title, user_direct),
                PendingHostEvent::TabCreated {
                    tab_id,
                    pane_id,
                    workspace_id,
                    kind,
                } => tab::emit_created(mgr, lua, tab_id, pane_id, workspace_id, kind),
                PendingHostEvent::TabClosed { tab_id, pane_id } => {
                    tab::emit_closed(mgr, lua, tab_id, pane_id)
                }
                PendingHostEvent::TabMoved {
                    tab_id,
                    from_pane,
                    to_pane,
                } => tab::emit_moved(mgr, tab_id, from_pane, to_pane),
                PendingHostEvent::PaneCreated {
                    pane_id,
                    workspace_id,
                } => pane::emit_created(mgr, lua, pane_id, workspace_id),
                PendingHostEvent::PaneClosed { pane_id } => pane::emit_closed(mgr, lua, pane_id),
                PendingHostEvent::PaneSplit {
                    original_pane,
                    new_pane,
                    direction,
                } => pane::emit_split(mgr, original_pane, new_pane, direction),
                PendingHostEvent::ProcessExited { surface_id } => {
                    misc::emit_process_exited(mgr, surface_id)
                }
                PendingHostEvent::NotificationCreated {
                    id,
                    title,
                    body,
                    source,
                } => misc::emit_notification_created(mgr, id, title, body, source),
                PendingHostEvent::HookFired {
                    hook_id,
                    event_kind,
                    surface_id,
                } => misc::emit_hook_fired(mgr, hook_id, event_kind, surface_id),
                PendingHostEvent::Raw { key, payload } => misc::emit_raw(mgr, key, payload),
            }
        }
    }
}
