use super::*;

fn mirror(id: u32, name: &str) -> crate::model::Workspace {
    let mut workspace =
        crate::model::Workspace::new_with_terminal_marker(id, name.into(), id + 1, id + 2, id + 3);
    workspace.mirror = true;
    workspace
}

fn resolve_without_executing(journal: &mut JournalApplication, session: &mut EngineSession) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        journal.poll_bootstrap(&mut [session]).unwrap();
        let requests = journal.requests_needing_resolution();
        if !requests.is_empty() {
            for (ticket, _) in requests {
                journal.resolve_ipc_for_engine(ticket, session);
            }
            return;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn workspace_response_is_frozen_and_host_notification_is_emitted_once() {
    let (mut session, mut journal) = boot();
    let workspace = session.core_state.local_workspaces[0].id;
    let sid = session.core_state.local_workspaces[0].all_surface_ids()[0];
    let generation = session.runtime.terminals.generation(sid);
    let initial = request(
        "workspace.update",
        serde_json::json!({"id": workspace, "name": "first", "subtitle":"subtitle"}),
        Some("workspace-meta"),
        1,
    );
    let counts_before = crate::ipc::handler::idempotency::retry_counts();
    let rx = send(&mut journal, initial.clone());
    let response = finish(&mut journal, &mut session, &rx);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(response.result.as_ref().unwrap()["name"], "first");
    assert_eq!(session.core_state.local_workspaces[0].name, "first");
    assert_eq!(
        session.runtime.terminals.generation(sid),
        generation,
        "metadata preserves the physical owner"
    );
    assert!(
        matches!(journal.commands.completed_host_events.as_slice(), [(engine, crate::core::host_event::PendingHostEvent::WorkspaceRenamed { workspace_id, name: Some(name), user_direct: false, .. })] if *engine == session.id && *workspace_id == workspace && name == "first")
    );
    journal.commands.completed_host_events.clear();
    let rx = send(
        &mut journal,
        request(
            "workspace.update",
            serde_json::json!({"id": workspace,"name":"second"}),
            None,
            2,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    journal.commands.completed_host_events.clear();
    let rx = send(&mut journal, initial);
    let replay = finish(&mut journal, &mut session, &rx);
    assert!(replay.idempotent_replay);
    let counts_after = crate::ipc::handler::idempotency::retry_counts();
    assert!(counts_after.executed > counts_before.executed);
    assert!(counts_after.replayed > counts_before.replayed);
    assert_eq!(replay.result, response.result);
    assert_eq!(session.core_state.local_workspaces[0].name, "second");
    assert!(journal.commands.completed_host_events.is_empty());
}

#[test]
fn mirror_delta_and_reconnect_replacement_cancel_stale_annotations_and_retries_do_not_reapply() {
    let (mut session, mut journal) = boot();
    session
        .core_state
        .push_mirror_workspace(mirror(900, "remote-initial"));
    for (index, replacement) in ["delta-name", "reconnected-name"].into_iter().enumerate() {
        let original = request(
            "workspace.update",
            serde_json::json!({"id":900, "name":"late-name","subtitle":"late-subtitle"}),
            Some(&format!("mirror-{index}")),
            index as u64,
        );
        let rx = send(&mut journal, original.clone());
        resolve_without_executing(&mut journal, &mut session);
        assert_eq!(session.core_state.mirror_workspaces[0].subtitle, "");
        // Both production delta and reconnect replace through this exact owner boundary.
        assert!(
            session
                .core_state
                .replace_mirror_workspace(mirror(900, replacement))
                .is_ok()
        );
        let response = finish(&mut journal, &mut session, &rx);
        assert!(response.error.is_none());
        assert_eq!(session.core_state.mirror_workspaces[0].name, replacement);
        assert_eq!(session.core_state.mirror_workspaces[0].subtitle, "");
        assert!(journal.commands.completed_host_events.is_empty());
        let rx = send(&mut journal, original);
        assert!(finish(&mut journal, &mut session, &rx).idempotent_replay);
        assert_eq!(session.core_state.mirror_workspaces[0].name, replacement);
    }
    let rx = send(
        &mut journal,
        request(
            "workspace.update",
            serde_json::json!({"id":900, "name":"current"}),
            None,
            5,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    assert_eq!(session.core_state.mirror_workspaces[0].name, "current");
    assert_eq!(journal.commands.completed_host_events.len(), 1);
    assert!(
        session
            .core_state
            .local_workspaces
            .iter()
            .all(|workspace| workspace.id != 900)
    );
}

#[test]
fn mixed_order_keeps_local_canonical_order_and_stored_move_does_not_move_again() {
    let (mut session, mut journal) = boot();
    let local = session.core_state.local_workspaces[0].id;
    let ticket = journal.next_ticket;
    journal.next_ticket += 1;
    let creation = crate::app::journal::creation::Creation::default_workspace(
        ticket,
        &session,
        &journal.worker,
    )
    .unwrap();
    journal.creations.insert(session.id, creation);
    let until = Instant::now() + Duration::from_secs(10);
    while !journal.creations.is_empty() {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let other = session.core_state.local_workspaces[1].id;
    session
        .core_state
        .push_mirror_workspace(mirror(900, "remote"));
    assert!(
        session
            .core_state
            .apply_workspace_display_order(vec![local, 900, other])
    );
    let original = request(
        "workspace.move",
        serde_json::json!({"from_index":0,"to_index":2}),
        Some("mixed-order"),
        1,
    );
    let rx = send(&mut journal, original.clone());
    let result = finish(&mut journal, &mut session, &rx);
    assert_eq!(result.result.unwrap()["moved"], true);
    assert_eq!(
        session
            .core_state
            .workspaces()
            .iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>(),
        [900, other, local]
    );
    assert_eq!(
        session
            .core_state
            .local_workspaces
            .iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>(),
        [other, local]
    );
    let binding = session.journal_binding.as_ref().unwrap();
    let store = tasty_event_store::EventStore::open(
        &tasty_utils::path::tasty_home()
            .unwrap()
            .join("structure/journal.db"),
        &binding.journal_id,
    )
    .unwrap();
    let model =
        crate::runtime::journal::load(&store, &tasty_event_store::StreamId::new(&binding.stream))
            .unwrap();
    assert_eq!(model.workspace_order, [other, local]);
    assert!(!model.workspaces.contains_key(&900));
    drop(store);
    let rx = send(
        &mut journal,
        request(
            "workspace.move",
            serde_json::json!({"id":local,"to_index":0}),
            None,
            2,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    let rx = send(&mut journal, original);
    assert!(finish(&mut journal, &mut session, &rx).idempotent_replay);
    assert_eq!(session.core_state.workspace_at(0).unwrap().id, local);
}
