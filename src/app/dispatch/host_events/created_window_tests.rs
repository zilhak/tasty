//! workspace.created의 window_id는 발행 시점에 그 workspace를 가진 창이며, 없으면 0이다.
//! 쌓인 사건에는 window_id가 없고, 발행 경로는 created_window로만 payload 값을 만들 수 있다.

use crate::core::engine_access::EngineMut;
use winit::window::WindowId;

use super::workspace::created_window;
use crate::app::engine_registry::EngineRegistry;
use crate::core::CoreState;
use crate::state::{MainViewState, PendingHostEvent};

const WINDOW: u64 = 42;
const OTHER_WINDOW: u64 = 7;

fn created_workspace_ids(state: &mut MainViewState) -> Vec<u32> {
    state
        .take_pending_host_events()
        .into_iter()
        .filter_map(|ev| match ev {
            PendingHostEvent::WorkspaceCreated { workspace_id, .. } => Some(workspace_id),
            _ => None,
        })
        .collect()
}

fn create_from_the_ui(state: &mut MainViewState, engine: &mut EngineMut<'_>) -> u32 {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    state.take_pending_host_events();
    let intent = crate::intent::Intent::NewWorkspace {
        kind: None,
        params: serde_json::json!({}),
        category: None,
    }
    .from_user_menu("test");
    crate::intent::workspace::handle(&mut core, state, engine, &intent);
    let ids = created_workspace_ids(state);
    assert_eq!(ids.len(), 1, "{ids:?}");
    ids[0]
}

fn create_from_ipc(state: &mut MainViewState, engine: &mut EngineMut<'_>) -> u32 {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    state.take_pending_host_events();
    let response = crate::adapters::ipc::handler::workspace::handle_workspace_create(
        &mut core,
        state,
        engine,
        serde_json::json!(1),
        &serde_json::json!({}),
    );
    assert!(response.error.is_none(), "{response:?}");
    let ids = created_workspace_ids(state);
    assert_eq!(ids.len(), 1, "{ids:?}");
    ids[0]
}

/// engine을 창 하나에 붙인 registry. 발행 경로가 보는 창 목록과 같은 순회를 쓴다.
fn registry_with_window(engine: CoreState, window: u64) -> EngineRegistry {
    let mut reg = EngineRegistry::default();
    let id = reg
        .insert_pending(engine)
        .unwrap_or_else(|_| panic!("pending engine already present"));
    reg.attach_window(WindowId::from(window), id);
    reg
}

fn window_id_at_emission(reg: &EngineRegistry, windows: &[u64], workspace_id: u32) -> u64 {
    let order = windows
        .iter()
        .map(|w| WindowId::from(*w))
        .collect::<Vec<_>>();
    created_window(reg.windows_in(order.into_iter()), workspace_id).id()
}

#[test]
fn a_ui_new_workspace_in_a_window_reports_that_window() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let ws = create_from_the_ui(&mut state, &mut engine);
    let reg = registry_with_window(engine, WINDOW);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), WINDOW);
}

#[test]
fn an_ipc_workspace_create_in_a_window_reports_that_window() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let ws = create_from_ipc(&mut state, &mut engine);
    let reg = registry_with_window(engine, WINDOW);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), WINDOW);
}

#[test]
fn only_the_window_that_owns_the_workspace_is_reported() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let ws = create_from_the_ui(&mut state, &mut engine);
    let (_, other_engine) = crate::state::tests::test_state();
    let mut reg = registry_with_window(other_engine, OTHER_WINDOW);
    let id = reg
        .insert_pending(engine)
        .unwrap_or_else(|_| panic!("pending engine already present"));
    reg.attach_window(WindowId::from(WINDOW), id);
    assert_eq!(
        window_id_at_emission(&reg, &[OTHER_WINDOW, WINDOW], ws),
        WINDOW
    );
}

#[test]
fn a_workspace_in_a_parked_engine_reports_zero() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let ws = create_from_the_ui(&mut state, &mut engine);
    let mut reg = registry_with_window(engine, WINDOW);
    reg.park(WindowId::from(WINDOW), state);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), 0);
}

#[test]
fn a_workspace_closed_before_emission_reports_zero() {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let ws = create_from_ipc(&mut state, &mut engine);
    let idx = engine
        .workspaces
        .iter()
        .position(|w| w.id == ws)
        .expect("created workspace");
    engine.workspaces.remove(idx);
    let reg = registry_with_window(engine, WINDOW);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), 0);
}
