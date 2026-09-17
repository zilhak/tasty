//! View-independent PTY execution termination, shared by GUI and headless drains.
use crate::core::{Core, CoreState};
use crate::state::AppState;

pub(crate) fn handle(core: &mut Core, state: &mut AppState, engine: &mut CoreState, surface: u32) {
    // Only an explicit PTY exit reaches this path; local window absence is not an exit.
    if let Err(error) = engine.completion.exited(surface, "process-exit") {
        tracing::warn!("completion process exit queued for persistence recovery: {error}");
    }
    let fired = engine
        .hook_manager
        .check_and_fire(surface, &[tasty_hooks::HookEvent::ProcessExit]);
    let injector = core.host_ipc_injector.get().cloned();
    for hook in fired {
        crate::hook_handler::trigger::execute_binding(
            &hook.binding,
            injector.as_ref(),
            &hook.event,
            &hook.received,
            surface,
        );
        // Both hosts resolve task waiters from HookFired; GUI also broadcasts it.
        state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
            hook_id: hook.hook_id,
            event_kind: "process-exit".into(),
            surface_id: surface,
            exit_code: None,
        });
    }
    #[cfg(feature = "gui")]
    state.enqueue_host_event(crate::state::PendingHostEvent::ProcessExited {
        surface_id: surface,
    });
    // The established PTY-exit close policy removes topology/PTY/soft occupancy,
    // runs standard lifecycle cleanup and preserves the no-snapshot contract.
    // intent-exempt: explicit PTY exit cascade, not a new user or agent command
    state.close_surface_by_id_no_snapshot(engine, surface, true);
}
