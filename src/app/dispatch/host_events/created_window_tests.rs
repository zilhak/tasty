//! workspace.created의 window_id는 발행 시점에 그 workspace를 가진 창이며, 없으면 0이다.

use winit::window::WindowId;

use super::resolve_created_window;
use crate::state::PendingHostEvent;

const WINDOW: u64 = 42;

fn created(workspace_id: u32, window_id: u64) -> PendingHostEvent {
    PendingHostEvent::WorkspaceCreated {
        workspace_id,
        window_id,
        name: "ws".to_owned(),
    }
}

fn window_of(ev: &PendingHostEvent) -> u64 {
    match ev {
        PendingHostEvent::WorkspaceCreated { window_id, .. } => *window_id,
        other => panic!("expected WorkspaceCreated, got {other:?}"),
    }
}

#[test]
fn a_workspace_owned_by_a_window_reports_that_window() {
    let ev = resolve_created_window(created(5, 0), |ws| (ws == 5).then_some(WINDOW));
    assert_eq!(window_of(&ev), WINDOW);
}

#[test]
fn a_workspace_without_a_window_reports_zero() {
    let ev = resolve_created_window(created(5, 9), |_| None);
    assert_eq!(window_of(&ev), 0, "the queued value is not used");
}

#[test]
fn the_owning_window_wins_over_the_queued_value() {
    let ev = resolve_created_window(created(5, 9), |_| Some(WINDOW));
    assert_eq!(window_of(&ev), WINDOW);
    // 창 경로가 쌓은 정상값은 그대로 남는다.
    let ev = resolve_created_window(created(5, WINDOW), |_| Some(WINDOW));
    assert_eq!(window_of(&ev), WINDOW);
}

#[test]
fn other_events_pass_through_without_a_lookup() {
    let ev = resolve_created_window(
        PendingHostEvent::WorkspaceClosed { workspace_id: 5 },
        |_| panic!("only workspace.created looks up its window"),
    );
    assert!(matches!(
        ev,
        PendingHostEvent::WorkspaceClosed { workspace_id: 5 }
    ));
}

/// 쌓인 이벤트에서 workspace.created를 하나 꺼내 창 조회로 보정한다. 창 조회는
/// `App::find_main_with_workspace`처럼 이 engine을 가진 창 하나를 흉내 낸다.
fn resolved_created_events(
    state: &mut crate::state::AppState,
    engine: &crate::core::CoreState,
    window: Option<WindowId>,
) -> Vec<(u32, u64, u64)> {
    state
        .take_pending_host_events()
        .into_iter()
        .filter_map(|ev| {
            let queued = match &ev {
                PendingHostEvent::WorkspaceCreated {
                    workspace_id,
                    window_id,
                    ..
                } => (*workspace_id, *window_id),
                _ => return None,
            };
            let resolved = resolve_created_window(ev, |ws| {
                window.filter(|_| engine.has_workspace(ws)).map(u64::from)
            });
            Some((queued.0, queued.1, window_of(&resolved)))
        })
        .collect()
}

#[test]
fn a_ui_new_workspace_in_a_window_reports_that_window() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    state.take_pending_host_events();
    let intent = crate::intent::Intent::NewWorkspace {
        kind: None,
        params: serde_json::json!({}),
        category: None,
    }
    .from_user_menu("test");
    crate::intent::workspace::handle(&mut core, &mut state, &mut engine, &intent);

    let window = WindowId::from(WINDOW);
    let events = resolved_created_events(&mut state, &engine, Some(window));
    assert_eq!(events.len(), 1, "{events:?}");
    let (_, queued, resolved) = events[0];
    assert_eq!(queued, 0, "the UI path does not know its window");
    assert_eq!(resolved, u64::from(window));
}

#[test]
fn an_ipc_workspace_create_in_a_window_reports_that_window() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    state.take_pending_host_events();
    let response = crate::adapters::ipc::handler::workspace::handle_workspace_create(
        &mut core,
        &mut state,
        &mut engine,
        serde_json::json!(1),
        &serde_json::json!({}),
    );
    assert!(response.error.is_none(), "{response:?}");

    let window = WindowId::from(WINDOW);
    let events = resolved_created_events(&mut state, &engine, Some(window));
    assert_eq!(events.len(), 1, "{events:?}");
    let (_, queued, resolved) = events[0];
    assert_eq!(queued, 0, "the IPC path does not know its window");
    assert_eq!(resolved, u64::from(window));
}

#[test]
fn a_workspace_in_an_engine_without_a_window_reports_zero() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    state.take_pending_host_events();
    let intent = crate::intent::Intent::NewWorkspace {
        kind: None,
        params: serde_json::json!({}),
        category: None,
    }
    .from_agent_ipc();
    crate::intent::workspace::handle(&mut core, &mut state, &mut engine, &intent);

    let events = resolved_created_events(&mut state, &engine, None);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].2, 0);
}
