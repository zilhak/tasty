//! Workspace/category selection compatibility over committed value fixtures.
use super::*;
fn navigation_fixture(
    categories: &[u32],
) -> (
    RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    use tasty_core::DomainEvent as E;
    let mut events = Vec::new();
    for category in 0..=categories.iter().copied().max().unwrap_or(0) {
        events.push(E::CategoryCreated {
            id: category,
            name: format!("category-{category}"),
            index: category as usize,
        });
    }
    for (index, category) in categories.iter().enumerate() {
        let id = index as u32 + 1;
        events.push(E::WorkspaceCreated {
            id,
            name: format!("workspace-{id}"),
            category: *category,
            index,
            pane: id,
        });
        events.push(E::TabCreated {
            id,
            pane: id,
            index: 0,
            name: "Terminal".into(),
            surface: tasty_core::SurfaceSpec {
                id,
                kind: "terminal".into(),
                data: None,
            },
        });
    }
    let (mut state, session) = test_state_from_model(test_model(events));
    // Seed selection without inventing an earlier user visit in the destination category.
    state.set_active_workspace_index(&session.core_state, categories.len().saturating_sub(1));
    (state, session)
}

#[test]
fn switch_workspace_valid() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0]);
    let engine = engine_session.borrow_mut();
    assert_eq!(state.active_workspace_index(&engine), 1);

    state.switch_workspace(&engine.read(), 0);
    assert_eq!(state.active_workspace_index(&engine), 0);
}

#[test]
fn switch_workspace_out_of_range() {
    let (mut state, mut engine_session) = navigation_fixture(&[0]);
    let engine = engine_session.borrow_mut();
    state.switch_workspace(&engine.read(), 999);
    assert_eq!(state.active_workspace_index(&engine), 0);
}

#[test]
fn next_prev_workspace_single_category_wraps() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 0]);
    let engine = engine_session.borrow_mut();
    assert_eq!(engine.workspaces().len(), 3);

    state.switch_workspace(&engine.read(), 0); // active = A
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // B
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // C
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → A

    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // wrap → C
    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // B
}

#[test]
fn next_workspace_in_active_category_wraps_within_category_only() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1, 0, 1]);
    let engine = engine_session.borrow_mut();
    assert_eq!(engine.workspaces().len(), 4);

    state.switch_workspace(&engine.read(), 0);
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // C (B=1 건너뜀)
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → A
    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // wrap → C

    state.switch_workspace(&engine.read(), 1);
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 3); // D (C=2 건너뜀)
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // wrap → B
    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 3); // wrap → D
}

#[test]
fn crosses_category_off_keeps_local_wrap() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 1, 1]);
    let engine = engine_session.borrow_mut();
    assert!(
        !engine
            .runtime
            .settings
            .general
            .workspace_switch_crosses_category
    );

    state.switch_workspace(&engine.read(), 1); // active = B (normal 의 마지막)
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → A (normal 의 첫), work 로 넘어가지 않음
}

#[test]
fn crosses_category_on_next_lands_on_next_category_first() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 1, 1]);
    let mut engine = engine_session.borrow_mut();

    state.switch_workspace(&engine.read(), 3);
    state.switch_workspace(&engine.read(), 1); // active = B (normal 의 마지막)
    engine
        .runtime
        .settings
        .general
        .workspace_switch_crosses_category = true;

    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // work 의 first = C (D 의 last-active 아님)
}

#[test]
fn crosses_category_on_prev_lands_on_prev_category_last() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 1, 1]);
    let mut engine = engine_session.borrow_mut();
    engine
        .runtime
        .settings
        .general
        .workspace_switch_crosses_category = true;

    state.switch_workspace(&engine.read(), 2); // active = C (work 의 첫)
    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // normal 의 last = B
}

#[test]
fn crosses_category_on_wraps_across_full_category_list() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 1, 1]);
    let mut engine = engine_session.borrow_mut();
    engine
        .runtime
        .settings
        .general
        .workspace_switch_crosses_category = true;

    state.switch_workspace(&engine.read(), 3); // active = D (마지막 카테고리의 마지막)
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → normal 의 first = A
}

#[test]
fn crosses_category_on_single_category_falls_back_to_local_wrap() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0, 0]);
    let mut engine = engine_session.borrow_mut();
    assert!(!engine.runtime.settings.general.workspace_categories_enabled);
    engine
        .runtime
        .settings
        .general
        .workspace_switch_crosses_category = true;

    state.switch_workspace(&engine.read(), 2); // active = C (normal 의 마지막, 유일한 카테고리)
    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → A, off 일 때와 동일
}

#[test]
fn switch_to_category_lands_on_last_active() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1, 0, 1]);
    let engine = engine_session.borrow_mut();

    state.switch_workspace(&engine.read(), 3);
    state.switch_workspace(&engine.read(), 0);
    assert_eq!(state.active_workspace_index(&engine), 0);

    state.switch_to_category(&engine.read(), 1);
    assert_eq!(state.active_workspace_index(&engine), 3);
}

#[test]
fn switch_to_category_falls_back_to_first_when_never_visited() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1, 0, 1]);
    let engine = engine_session.borrow_mut();

    state.switch_workspace(&engine.read(), 0);
    state.switch_to_category(&engine.read(), 1);
    assert_eq!(state.active_workspace_index(&engine), 1);
}

#[cfg(feature = "gui")]
#[test]
fn switch_to_category_auto_expands_collapsed() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1]);
    let mut engine = engine_session.borrow_mut();
    let work = 1;
    state.navigation.collapsed_categories.insert(work);
    assert!(state.navigation.collapsed_categories.contains(&work));

    state.switch_workspace(&engine.read(), 0); // active=A(normal)
    state.switch_to_category(&engine.read(), 1); // → work
    // 펼침은 View UI 요청으로 큐에 들어가므로 메인 루프처럼 큐를 비운다.
    for intent in state.take_pending_intents() {
        crate::intent::popup::handle(&mut state, &mut engine, &intent);
    }
    assert!(!state.navigation.collapsed_categories.contains(&work)); // auto-expand
    assert_eq!(state.active_workspace_index(&engine), 1); // work first = B
}

#[test]
fn switch_to_category_out_of_range_noop() {
    let (mut state, mut engine_session) = navigation_fixture(&[0]);
    let engine = engine_session.borrow_mut();
    state.switch_workspace(&engine.read(), 0);
    state.switch_to_category(&engine.read(), 99);
    assert_eq!(state.active_workspace_index(&engine), 0);
}

#[test]
fn next_prev_category_wraps_across_categories() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1, 2]);
    let engine = engine_session.borrow_mut();

    state.switch_workspace(&engine.read(), 0); // active = A (normal)
    state.next_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // work → B
    state.next_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // play → C
    state.next_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0); // wrap → normal → A

    state.prev_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 2); // wrap → play → C
    state.prev_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1); // work → B
}

#[test]
fn next_prev_category_noop_when_single_category() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 0]);
    let engine = engine_session.borrow_mut();
    assert_eq!(state.active_workspace_index(&engine), 1);

    state.next_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1);
    state.prev_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 1);
}

#[test]
fn next_category_lands_on_last_active() {
    let (mut state, mut engine_session) = navigation_fixture(&[0, 1, 1]);
    let engine = engine_session.borrow_mut();

    state.switch_workspace(&engine.read(), 2);
    state.switch_workspace(&engine.read(), 0); // normal 로 복귀.

    state.next_category(&engine.read()); // → work, last-active = C.
    assert_eq!(state.active_workspace_index(&engine), 2);
}

#[test]
fn next_prev_workspace_in_active_category_noop_when_alone() {
    let (mut state, mut engine_session) = navigation_fixture(&[0]);
    let engine = engine_session.borrow_mut();
    assert_eq!(engine.workspaces().len(), 1);
    assert_eq!(state.active_workspace_index(&engine), 0);

    state.next_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0);
    state.prev_workspace_in_active_category(&engine.read());
    assert_eq!(state.active_workspace_index(&engine), 0);
}
