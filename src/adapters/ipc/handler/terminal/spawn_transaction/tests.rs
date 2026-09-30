use super::*;

#[test]
fn send_failure_after_registry_and_soft_lock_closes_only_the_owned_surface() {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let parent = engine.workspaces[0].all_surface_ids()[0];
    let pane = engine.workspaces[0].pane_layout().all_pane_ids()[0];
    // An owned empty surface cannot accept a terminal command. This injects a
    // real send failure at the last step of `finish`, after the child registry
    // entry and the soft acquisition are both already in place.
    let created = tab::handle_tab_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({"pane_id":pane,"type":"empty"}),
        &crate::core::origin::IPC_AGENT,
    );
    let target = created.result.unwrap()["surface_id"].as_u64().unwrap() as u32;
    let child = ChildEntry {
        child_surface_id: target,
        index: 0,
        cwd: None,
        role: Some("worker".into()),
        nickname: None,
    };
    state.pending_lifecycle_events.clear();
    let result = finish(
        &mut core,
        &mut state,
        &mut engine,
        &json!(2),
        parent,
        child,
        Some("must not run"),
        &crate::core::origin::IPC_AGENT,
    );
    assert!(result.is_err());
    assert!(engine.find_surface_by_id(target).is_none());
    assert!(engine.find_surface_by_id(parent).is_some());
    assert!(engine.attach.occupancy_of(target).is_none());
    assert!(
        engine
            .runtime
            .child_terminals
            .list_children(parent)
            .is_empty()
    );
    // 롤백 닫기는 spawn 요청의 출처를 따른다.
    assert!(!state.pending_lifecycle_events.is_empty());
    assert!(
        state
            .pending_lifecycle_events
            .iter()
            .all(|e| !e.is_user_close)
    );
}
