use super::*;

#[test]
fn failure_after_relation_commit_closes_only_the_owned_surface_and_soft_lock() {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let parent = engine.workspaces[0].all_surface_ids()[0];
    let pane = engine.workspaces[0].pane_layout().all_pane_ids()[0];
    // An owned empty surface cannot accept a terminal command. This injects a
    // real send failure after relationship commit, registry and soft acquisition.
    let created = tab::handle_tab_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({"pane_id":pane,"type":"empty"}),
    );
    let target = created.result.unwrap()["surface_id"].as_u64().unwrap() as u32;
    let child = ChildEntry {
        child_surface_id: target,
        index: 0,
        cwd: None,
        role: Some("worker".into()),
        nickname: None,
    };
    let result = finish(
        &mut core,
        &mut state,
        &mut engine,
        &json!(2),
        parent,
        child,
        Some("must not run"),
    );
    assert!(result.is_err());
    assert!(engine.find_surface_by_id(target).is_none());
    assert!(engine.find_surface_by_id(parent).is_some());
    assert!(engine.attach.occupancy_of(target).is_none());
    assert!(engine.child_terminals.list_children(parent).is_empty());
    assert!(
        engine
            .completion
            .snapshot()
            .unwrap()
            .live_relations
            .is_empty()
    );
}

#[test]
fn failed_adopt_commit_preserves_preexisting_same_parent_soft_ownership() {
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let parent = engine.workspaces[0].all_surface_ids()[0];
    let pane = engine.workspaces[0].pane_layout().all_pane_ids()[0];
    let created = tab::handle_tab_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({"pane_id":pane,"type":"empty"}),
    );
    let target = created.result.unwrap()["surface_id"].as_u64().unwrap() as u32;
    engine
        .occupy_soft(target, parent, Some("original-label".into()))
        .unwrap();
    let before = engine.attach.occupancy_of(target).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reject.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE completion_journal (id INTEGER PRIMARY KEY CHECK(id=1),body TEXT NOT NULL); CREATE TRIGGER reject_relation BEFORE UPDATE ON completion_journal WHEN json_extract(NEW.body,'$.sequence') > json_extract(OLD.body,'$.sequence') BEGIN SELECT RAISE(ABORT,'owned relation fault'); END;").unwrap();
    drop(db);
    engine.completion = crate::core::completion::Completion::open(&path).unwrap();
    let response = handle_adopt(
        &mut engine,
        json!(2),
        &json!({"surface":parent,"target":target,"nickname":"replacement-label"}),
    );
    assert!(
        response
            .error
            .unwrap()
            .message
            .contains("owned relation fault")
    );
    assert_eq!(engine.attach.occupancy_of(target), Some(before));
    assert!(engine.child_terminals.list_children(parent).is_empty());
    assert!(engine.find_surface_by_id(target).is_some());
}
