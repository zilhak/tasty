//! 대기 중인 호스트 이벤트를 프로토콜 payload로 바꿔 전달한다.

mod misc;
mod pane;
mod surface;
mod tab;
mod workspace;

#[cfg(test)]
mod created_window_tests;

use crate::app::App;
use crate::app::window_access::engines_mut;
use crate::core::CoreState;
use crate::state::PendingHostEvent;

impl App {
    pub(crate) fn dispatch_pending_host_events(&mut self) {
        let mut drained: Vec<PendingHostEvent> = Vec::new();
        for (s, engine) in engines_mut!(self).sessions() {
            s.detect_focus_change(engine);
            s.detect_workspace_activation(engine);
            s.detect_tab_focus_change(engine);
            s.detect_tab_lifecycle(engine);
            let events = s.take_pending_host_events();
            reproject_osc_title_on_focus(engine, &events);
            resolve_hook_fired_task_waits(&self.core, engine, &events);
            drained.extend(events);
        }
        if drained.is_empty() {
            return;
        }
        // 수집하면서 창 구분이 사라지고 IPC·UI 경로는 창 id를 모른 채 0으로 쌓으므로, 발행 직전에
        // workspace를 가진 창을 다시 찾는다.
        let drained: Vec<PendingHostEvent> = drained
            .into_iter()
            .map(|ev| {
                resolve_created_window(ev, |workspace_id| {
                    self.find_main_with_workspace(workspace_id).map(u64::from)
                })
            })
            .collect();
        let scripts = self.autofire_scripts();
        macro_rules! af {
            () => {
                crate::hooks::lua::AutofireCtx {
                    scripts: &scripts,
                    guard: &mut self.lua_autofire,
                }
            };
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
                    af!(),
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
                    af!(),
                    workspace::RenameEvent {
                        workspace_id,
                        name,
                        subtitle,
                        description,
                        user_direct,
                    },
                ),
                PendingHostEvent::WorkspaceCreated {
                    workspace_id,
                    window_id,
                    name,
                } => workspace::emit_created(mgr, lua, af!(), workspace_id, window_id, name),
                PendingHostEvent::WorkspaceClosed { workspace_id } => {
                    workspace::emit_closed(mgr, lua, af!(), workspace_id)
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
                } => tab::emit_renamed(mgr, lua, af!(), tab_id, title, user_direct),
                PendingHostEvent::TabCreated {
                    tab_id,
                    pane_id,
                    workspace_id,
                    kind,
                } => tab::emit_created(mgr, lua, af!(), tab_id, pane_id, workspace_id, kind),
                PendingHostEvent::TabClosed { tab_id, pane_id } => {
                    tab::emit_closed(mgr, lua, af!(), tab_id, pane_id)
                }
                PendingHostEvent::TabMoved {
                    tab_id,
                    from_pane,
                    to_pane,
                } => tab::emit_moved(mgr, tab_id, from_pane, to_pane),
                PendingHostEvent::PaneCreated {
                    pane_id,
                    workspace_id,
                } => pane::emit_created(mgr, lua, af!(), pane_id, workspace_id),
                PendingHostEvent::PaneClosed { pane_id } => {
                    pane::emit_closed(mgr, lua, af!(), pane_id)
                }
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
                    exit_code: _,
                } => misc::emit_hook_fired(mgr, hook_id, event_kind, surface_id),
                PendingHostEvent::PluginLoaded { plugin_id, version } => {
                    misc::emit_plugin_loaded(mgr, plugin_id, version)
                }
                PendingHostEvent::PluginEnableToggled { plugin_id, enabled } => {
                    misc::emit_plugin_enable_toggled(mgr, plugin_id, enabled)
                }
                PendingHostEvent::PluginUnloaded { plugin_id, reason } => {
                    misc::emit_plugin_unloaded(mgr, plugin_id, reason)
                }
                PendingHostEvent::PluginError {
                    plugin_id,
                    error_kind,
                    message,
                } => misc::emit_plugin_error(mgr, plugin_id, error_kind, message),
                PendingHostEvent::PluginRegistryChanged {
                    plugin_id,
                    change_kind,
                    detail,
                } => misc::emit_plugin_registry_changed(mgr, plugin_id, change_kind, detail),
                PendingHostEvent::PluginSurfaceKindRegistered {
                    plugin_id,
                    kind,
                    rendering,
                } => misc::emit_plugin_surface_kind_registered(mgr, plugin_id, kind, rendering),
                PendingHostEvent::PluginWindowDeclared {
                    plugin_id,
                    window_id,
                } => misc::emit_plugin_window_declared(mgr, plugin_id, window_id),
            }
        }
    }
}

/// 포커스가 옮겨온 surface의 제목을 탭에 반영한다.
/// 배경 탭의 포커스 변경은 여기서 감지하지 않아 닫기·이동 경로가 직접 반영한다.
fn reproject_osc_title_on_focus(engine: &mut CoreState, events: &[PendingHostEvent]) {
    for ev in events {
        if let PendingHostEvent::SurfaceFocused { surface_id, .. } = ev {
            engine.refresh_tab_osc_title(*surface_id);
        }
    }
}

/// 이벤트를 다른 창의 이벤트와 합치기 전에 해당 engine으로 대기 작업을 완료한다.
/// 다른 engine을 넘기면 대기자를 깨울 waker hub가 달라진다.
fn resolve_hook_fired_task_waits(
    core: &crate::core::Core,
    engine: &CoreState,
    events: &[PendingHostEvent],
) {
    for ev in events {
        if let PendingHostEvent::HookFired {
            hook_id, exit_code, ..
        } = ev
        {
            core.tasks.resolve_hook_task_wait(
                &engine.task_scope,
                *hook_id,
                *exit_code,
                core.now_unix_millis() as u64,
            );
        }
    }
}

/// workspace.created의 window_id를 발행 시점에 그 workspace를 가진 창으로 정한다. 쌓을 때의 값은
/// 쓰지 않는다. 창이 없는 engine(parked)에 있거나 발행 전에 사라졌으면 0이다.
fn resolve_created_window(
    ev: PendingHostEvent,
    window_of: impl Fn(u32) -> Option<u64>,
) -> PendingHostEvent {
    match ev {
        PendingHostEvent::WorkspaceCreated {
            workspace_id, name, ..
        } => PendingHostEvent::WorkspaceCreated {
            workspace_id,
            window_id: window_of(workspace_id).unwrap_or(0),
            name,
        },
        other => other,
    }
}
