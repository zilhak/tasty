use super::*;
use crate::runtime::command_executor::GroupCommand;

fn group(
    executor: &Executor<Fake>,
    requests: &[Request<Cmd>],
) -> crate::runtime::command_executor::GroupExecution<Reject> {
    executor.execute_group(requests.len(), |index, _, _| {
        Ok(GroupCommand::Ready(requests[index].clone()))
    })
}

#[test]
fn later_decide_sees_private_predecessor_and_actual_batches_replay_to_the_same_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(&dir);
    let fake = Fake::default();
    let executor = open(&path, &fake);
    let before = executor.inner.lock().unwrap().store.commit_statistics();
    let result = group(
        &executor,
        &[
            request(Some("one"), create_cmd("first")),
            request(
                Some("rename"),
                Cmd::RenameWorkspace {
                    id: 1,
                    name: "renamed".into(),
                },
            ),
            request(Some("two"), create_cmd("second")),
        ],
    );
    assert!(result.halted.is_none());
    assert_eq!(result.results.len(), 3);
    assert!(
        result
            .results
            .iter()
            .all(|r| r.as_ref().is_ok_and(committed))
    );
    assert_eq!(
        result.results[2].as_ref().unwrap().response.as_deref(),
        Some(b"workspace:2".as_slice())
    );
    let after = executor.inner.lock().unwrap().store.commit_statistics();
    assert_eq!(after.physical_commits - before.physical_commits, 1);
    assert_eq!(after.logical_requests - before.logical_requests, 3);
    let model = state(&executor);
    assert_eq!(model.workspace_order, [1, 2]);
    assert_eq!(model.workspaces[&1].name, "renamed");
    drop(executor);
    let store = EventStore::open(&path, JOURNAL).unwrap();
    assert_eq!(store.read_batches_after(None, 10).unwrap().len(), 3);
    assert_eq!(
        journal::full_replay(&store)
            .unwrap()
            .stream(super::super::common::stream().as_str()),
        model
    );
}

#[test]
fn rejected_request_and_conflicting_key_do_not_leak_into_later_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let executor = open(&db_path(&dir), &Fake::default());
    let before = executor.inner.lock().unwrap().store.commit_statistics();
    let first = request(Some("one"), create_cmd("first"));
    let result = group(
        &executor,
        &[
            first.clone(),
            request(
                Some("bad"),
                Cmd::RenameWorkspace {
                    id: 99,
                    name: "missing".into(),
                },
            ),
            first,
            request(Some("one"), create_cmd("conflicting")),
            request(Some("two"), create_cmd("second")),
        ],
    );
    assert!(result.halted.is_none());
    assert!(matches!(
        result.results[1],
        Err(ExecError::Rejected(Reject::NoSuchWorkspace))
    ));
    assert_eq!(result.results[2].as_ref().unwrap().source, Source::Stored);
    assert_eq!(
        result.results[0].as_ref().unwrap().command_id,
        result.results[2].as_ref().unwrap().command_id
    );
    assert!(matches!(result.results[3], Err(ExecError::KeyConflict(_))));
    assert_eq!(state(&executor).workspace_order, [1, 2]);
    let inner = executor.inner.lock().unwrap();
    assert_eq!(inner.store.read_batches_after(None, 10).unwrap().len(), 2);
    assert_eq!(
        inner.store.commit_statistics().physical_commits - before.physical_commits,
        1
    );
}

#[test]
fn sql_failure_discards_staged_state_and_all_success_results_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(&dir);
    let fake = Fake::default();
    let executor = open(&path, &fake);
    let before = state(&executor);
    let requests = ["one", "two"].map(|name| {
        request(
            Some(name),
            Cmd::CreateWithEffect {
                name: name.into(),
                effect_id: "same-effect".into(),
            },
        )
    });
    let result = group(&executor, &requests);
    assert!(result.halted.is_some());
    assert_eq!(result.results.len(), 2);
    assert!(result.results.iter().all(Result::is_err));
    assert_eq!(state(&executor), before);
    assert_eq!(fake.decides(), 2);
    assert!(matches!(
        executor.execute(&request(Some("later"), create_cmd("later"))),
        Err(ExecError::Halted)
    ));
    drop(executor);
    let store = EventStore::open(&path, JOURNAL).unwrap();
    assert!(store.read_batches_after(None, 10).unwrap().is_empty());
    assert!(store.effect("same-effect").unwrap().is_none());
}

#[test]
fn failed_private_apply_leaves_neither_published_state_nor_durable_facts() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let executor = open(&db_path(&dir), &fake);
    let before = state(&executor);
    fake.fail_apply.store(true, Ordering::SeqCst);
    let result = group(&executor, &[request(Some("one"), create_cmd("first"))]);
    assert!(result.halted.is_some());
    assert!(matches!(result.results[0], Err(ExecError::Apply(_))));
    assert_eq!(state(&executor), before);
    assert!(
        executor
            .inner
            .lock()
            .unwrap()
            .store
            .read_batches_after(None, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn known_revision_conflict_retries_only_the_rolled_back_group() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let executor = open(&db_path(&dir), &fake);
    fake.stale_revisions.store(1, Ordering::SeqCst);
    let result = group(
        &executor,
        &[
            request(Some("one"), create_cmd("first")),
            request(Some("two"), create_cmd("second")),
        ],
    );
    assert!(result.halted.is_none());
    assert!(result.results.iter().all(Result::is_ok));
    assert_eq!(fake.decides(), 3);
    assert_eq!(state(&executor).workspace_order, [1, 2]);
    assert_eq!(
        executor
            .inner
            .lock()
            .unwrap()
            .store
            .commit_statistics()
            .structural_commits,
        1
    );
}

#[test]
fn completion_builder_can_return_its_first_stored_record_without_deciding() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let executor = open(&db_path(&dir), &fake);
    let first = executor
        .execute(&request(Some("one"), create_cmd("first")))
        .unwrap();
    let before = executor.inner.lock().unwrap().store.commit_statistics();
    let result = executor.execute_group(1, |_, _, store| {
        Ok(GroupCommand::Stored(
            store.command(&first.command_id).unwrap().unwrap(),
        ))
    });
    assert!(result.halted.is_none());
    let result = result.results[0].as_ref().unwrap();
    assert_eq!(result.source, Source::Stored);
    assert_eq!(result.command_id, first.command_id);
    assert_eq!(result.response, first.response);
    assert_eq!(fake.decides(), 1);
    assert_eq!(
        executor.inner.lock().unwrap().store.commit_statistics(),
        before
    );
}

#[test]
fn a_group_joins_the_existing_leader_even_when_it_fails_without_a_durable_record() {
    for rejects in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let gate = Arc::new(Gate::default());
        let fake = Fake {
            gate: Some(gate.clone()),
            ..Fake::default()
        };
        let executor = Arc::new(open(&db_path(&dir), &fake));
        let first = request(
            Some("shared"),
            if rejects {
                Cmd::RenameWorkspace {
                    id: 99,
                    name: "missing".into(),
                }
            } else {
                create_cmd("first")
            },
        );
        let lead = executor.clone();
        let leader_request = first.clone();
        let leader = std::thread::spawn(move || lead.execute(&leader_request));
        wait_until(|| gate.entered.load(Ordering::SeqCst));
        let grouped = executor.clone();
        let group_thread = std::thread::spawn(move || {
            group(
                &grouped,
                &[first, request(Some("shared"), create_cmd("conflicting"))],
            )
        });
        wait_until(|| executor.group_waiting_for_in_flight.load(Ordering::SeqCst));
        gate.release.store(true, Ordering::SeqCst);
        let leader_result = leader.join().unwrap();
        let grouped = group_thread.join().unwrap();
        assert!(grouped.halted.is_none());
        assert_eq!(grouped.results.len(), 2);
        if rejects {
            assert!(matches!(
                leader_result,
                Err(ExecError::Rejected(Reject::NoSuchWorkspace))
            ));
            assert!(matches!(
                grouped.results[0],
                Err(ExecError::Rejected(Reject::NoSuchWorkspace))
            ));
        } else {
            let leader = leader_result.unwrap();
            let joined = grouped.results[0].as_ref().unwrap();
            assert_eq!(joined.source, Source::Joined);
            assert_eq!(joined.command_id, leader.command_id);
            assert_eq!(joined.response, leader.response);
        }
        assert!(matches!(grouped.results[1], Err(ExecError::KeyConflict(_))));
        assert_eq!(
            fake.decides(),
            1,
            "a concurrent failed leader must not be automatically retried by the group"
        );
        assert_eq!(
            executor
                .inner
                .lock()
                .unwrap()
                .store
                .commit_statistics()
                .structural_commits,
            if rejects { 0 } else { 1 }
        );
    }
}

#[test]
fn physical_commit_failure_discards_private_state_and_requires_reopen_without_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(&dir);
    drop(EventStore::open(&path, JOURNAL).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE group_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE group_child(parent INTEGER REFERENCES group_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER group_commit_failure AFTER INSERT ON commands BEGIN INSERT INTO group_child VALUES(999); END;").unwrap();
    drop(conn);
    let fake = Fake::default();
    let executor = open(&path, &fake);
    let before = state(&executor);
    let stats = executor.inner.lock().unwrap().store.commit_statistics();
    let result = group(
        &executor,
        &[
            request(Some("one"), create_cmd("first")),
            request(Some("two"), create_cmd("second")),
        ],
    );
    assert!(
        result
            .halted
            .as_deref()
            .unwrap()
            .contains("durable outcome")
    );
    assert!(result.results.iter().all(Result::is_err));
    assert_eq!(fake.decides(), 2);
    assert_eq!(state(&executor), before);
    let inner = executor.inner.lock().unwrap();
    let after = inner.store.commit_statistics();
    assert_eq!(after.physical_commits, stats.physical_commits);
    assert_eq!(after.failed_commits, stats.failed_commits + 1);
    assert!(inner.store.read_batches_after(None, 10).unwrap().is_empty());
}

#[test]
fn grouped_history_matches_single_execution_with_only_recorded_time_normalized() {
    let one = tempfile::tempdir().unwrap();
    let many = tempfile::tempdir().unwrap();
    let single = open(&db_path(&one), &Fake::default());
    let grouped = open(&db_path(&many), &Fake::default());
    let first = request(Some("first"), create_cmd("first"));
    let requests = [
        first.clone(),
        request(
            Some("rename"),
            Cmd::RenameWorkspace {
                id: 1,
                name: "renamed".into(),
            },
        ),
        request(
            Some("rejected"),
            Cmd::RenameWorkspace {
                id: 99,
                name: "missing".into(),
            },
        ),
        first,
        request(
            Some("effect"),
            Cmd::CreateWithEffect {
                name: "second".into(),
                effect_id: "same-effect".into(),
            },
        ),
    ];
    let singles: Vec<_> = requests
        .iter()
        .map(|request| single.execute(request))
        .collect();
    let batch = group(&grouped, &requests);
    assert!(batch.halted.is_none());
    for (single, grouped) in singles.iter().zip(&batch.results) {
        match (single, grouped) {
            (Ok(single), Ok(grouped)) => assert_eq!(single, grouped),
            (Err(single), Err(grouped)) => assert_eq!(single.to_string(), grouped.to_string()),
            other => panic!("single/group outcome differs: {other:?}"),
        }
    }
    assert_eq!(state(&single), state(&grouped));
    let single = single.inner.lock().unwrap();
    let grouped = grouped.inner.lock().unwrap();
    // Epochs, IDs and input order are controlled; wall-clock event timestamps are the only mapping.
    let normalize = |store: &EventStore| {
        let mut batches = store.read_batches_after(None, 100).unwrap();
        for batch in &mut batches {
            for event in &mut batch.events {
                event.recorded_at_ms = 0;
            }
        }
        batches
    };
    assert_eq!(normalize(&single.store), normalize(&grouped.store));
    for (name, index) in [("first", 0), ("rename", 1), ("rejected", 2), ("effect", 4)] {
        let digest = Fake::default().request_digest(&requests[index].command);
        assert_eq!(
            single.store.lookup_command(&key(name), &digest).unwrap(),
            grouped.store.lookup_command(&key(name), &digest).unwrap()
        );
    }
    assert_eq!(
        single.store.effect("same-effect").unwrap(),
        grouped.store.effect("same-effect").unwrap()
    );
    assert_eq!(single.store.commit_statistics().structural_commits, 3);
    assert_eq!(grouped.store.commit_statistics().structural_commits, 1);
    assert_eq!(
        single.store.commit_statistics().logical_requests,
        grouped.store.commit_statistics().logical_requests
    );
}
