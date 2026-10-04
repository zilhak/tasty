//! workspace.created의 window_id는 발행 시점에 그 workspace를 가진 창이며, 없으면 0이다.
//! 쌓인 사건에는 window_id가 없고, 발행 경로는 created_window로만 payload 값을 만들 수 있다.

use winit::window::WindowId;

use super::workspace::created_window;
use crate::app::engine_registry::EngineRegistry;

const WINDOW: u64 = 42;
const OTHER_WINDOW: u64 = 7;

/// engine을 창 하나에 붙인 registry. 발행 경로가 보는 창 목록과 같은 순회를 쓴다.
fn registry_with_window(
    engine: crate::runtime::engine_session::EngineSession,
    window: u64,
) -> EngineRegistry {
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
    created_window(
        reg.windows_in(order.into_iter())
            .map(|(wid, e)| (wid, e.core)),
        workspace_id,
    )
    .id()
}

#[test]
fn a_committed_workspace_in_a_window_reports_that_window() {
    let (_, engine_session) = crate::state::tests::test_state();
    let ws = engine_session.core_state.workspace_at(0).unwrap().id;
    let reg = registry_with_window(engine_session, WINDOW);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), WINDOW);
}

#[test]
fn only_the_window_that_owns_the_workspace_is_reported() {
    let (_, engine_session) = crate::state::tests::test_state();
    let ws = engine_session.core_state.workspace_at(0).unwrap().id;
    let (_, other_engine_session) = crate::state::tests::test_state();
    let mut reg = registry_with_window(other_engine_session, OTHER_WINDOW);
    let id = reg
        .insert_pending(engine_session)
        .unwrap_or_else(|_| panic!("pending engine already present"));
    reg.attach_window(WindowId::from(WINDOW), id);
    assert_eq!(
        window_id_at_emission(&reg, &[OTHER_WINDOW, WINDOW], ws),
        WINDOW
    );
}

#[test]
fn a_workspace_in_a_parked_engine_reports_zero() {
    let (state, engine_session) = crate::state::tests::test_state();
    let ws = engine_session.core_state.workspace_at(0).unwrap().id;
    let mut reg = registry_with_window(engine_session, WINDOW);
    reg.park(WindowId::from(WINDOW), state);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), 0);
}

#[test]
fn a_workspace_closed_before_emission_reports_zero() {
    let (_, mut engine_session) = crate::state::tests::test_state();
    let ws = engine_session.core_state.workspace_at(0).unwrap().id;
    engine_session.core_state = crate::core::CoreState::new_base();
    let reg = registry_with_window(engine_session, WINDOW);
    assert_eq!(window_id_at_emission(&reg, &[WINDOW], ws), 0);
}
