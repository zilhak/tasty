use super::common::{append, command, fresh, key};
use crate::{CommitOutcome, CommitRequest, ExpectedRevision, StoreError, StreamId};

fn request(epoch: crate::WriterEpoch, n: u64) -> CommitRequest {
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command(
        &format!("c{n}"),
        Some(key("group", &n.to_string())),
        &[n as u8],
    ));
    request.appends.push(append(
        "a",
        if n == 1 {
            ExpectedRevision::NoStream
        } else {
            ExpectedRevision::Exact(n - 1)
        },
        &[&format!("e{n}")],
    ));
    request
}

#[test]
fn group_shares_only_the_physical_commit_and_reads_its_actual_cuts() {
    let (_dir, mut store, epoch) = fresh();
    let before = store.commit_statistics();
    let mut group = store.begin_commit_group(epoch).unwrap();
    let mut cuts = Vec::new();
    for n in 1..=3 {
        let CommitOutcome::Committed { batch: Some(cut) } =
            group.append(&request(epoch, n)).unwrap()
        else {
            panic!("batch");
        };
        assert_eq!(cut.revisions[&StreamId::new("a")], n);
        assert_eq!(group.read_batch(cut.batch_id).unwrap().events.len(), 1);
        cuts.push(cut.batch_id);
    }
    group.finish().unwrap();
    assert!(cuts.windows(2).all(|w| w[0] < w[1]));
    let after = store.commit_statistics();
    assert_eq!(after.physical_commits - before.physical_commits, 1);
    assert_eq!(after.logical_requests - before.logical_requests, 3);
    assert_eq!(after.structural_commits - before.structural_commits, 1);
    for n in 1..=3 {
        let saved = store.command(&format!("c{n}")).unwrap().unwrap();
        assert_eq!(saved.batch_ids, [cuts[n as usize - 1]]);
        assert_eq!(saved.response.as_deref(), Some(b"ok".as_slice()));
    }
}

#[test]
fn savepoint_removes_a_rejected_partial_request_and_later_request_can_continue() {
    let (_dir, mut store, epoch) = fresh();
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&request(epoch, 1)).unwrap();
    let mut invalid = request(epoch, 2);
    invalid.command.as_mut().unwrap().command_id = "discarded".into();
    invalid.appends[0].expected = ExpectedRevision::NoStream;
    assert!(matches!(
        group.append(&invalid),
        Err(StoreError::RevisionConflict { .. })
    ));
    group.append(&request(epoch, 2)).unwrap();
    group.finish().unwrap();
    assert_eq!(store.command("discarded").unwrap(), None);
    assert_eq!(store.stream_revision(&StreamId::new("a")).unwrap(), Some(2));
}

#[test]
fn duplicate_only_group_does_not_commit_and_conflicting_key_does_not_overwrite() {
    let (_dir, mut store, epoch) = fresh();
    let first = request(epoch, 1);
    store.commit(&first).unwrap();
    let before = store.commit_statistics();
    let mut group = store.begin_commit_group(epoch).unwrap();
    assert!(matches!(
        group.append(&first),
        Ok(CommitOutcome::Duplicate(_))
    ));
    let mut conflicting = first.clone();
    conflicting.command.as_mut().unwrap().request_digest = b"different".to_vec();
    assert!(matches!(
        group.append(&conflicting),
        Err(StoreError::KeyConflict { .. })
    ));
    group.finish().unwrap();
    assert_eq!(store.commit_statistics(), before);
}

#[test]
fn sql_failure_prevents_commit_of_any_previous_entry() {
    let (_dir, mut store, epoch) = fresh();
    let before = store.commit_statistics();
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&request(epoch, 1)).unwrap();
    let mut invalid = request(epoch, 2);
    invalid.command.as_mut().unwrap().command_id = "c1".into();
    assert!(matches!(group.append(&invalid), Err(StoreError::Sqlite(_))));
    assert!(matches!(
        group.append(&request(epoch, 2)),
        Err(StoreError::GroupAborted)
    ));
    assert!(matches!(group.finish(), Err(StoreError::GroupAborted)));
    assert_eq!(store.command("c1").unwrap(), None);
    assert_eq!(store.current_cut().unwrap().last_batch, None);
    assert_eq!(store.commit_statistics(), before);
}

#[test]
fn rollback_removes_event_and_pin_written_before_unknown_command_update() {
    let (_dir, mut store, epoch) = fresh();
    let reference = store.put_payload(epoch, b"pinned").unwrap();
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&request(epoch, 1)).unwrap();
    let mut invalid = request(epoch, 2);
    invalid.command.as_mut().unwrap().command_id = "partial".into();
    invalid.appends[0].events[0].event_id = "partial-event".into();
    invalid.appends[0].events[0].payload_refs.push(reference);
    invalid.command_updates.push(crate::CommandUpdate {
        command_id: "missing".into(),
        status: crate::CommandStatus::Completed,
        response: None,
    });
    assert!(matches!(
        group.append(&invalid),
        Err(StoreError::UnknownCommand(_))
    ));
    group.append(&request(epoch, 2)).unwrap();
    group.finish().unwrap();
    assert_eq!(store.command("partial").unwrap(), None);
    assert_eq!(store.stream_revision(&StreamId::new("a")).unwrap(), Some(2));
    let pins: u64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM payload_pins WHERE holder=?1",
            ["event:partial-event"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pins, 0);
}

#[test]
fn pending_effect_capacity_counts_successes_in_the_same_transaction() {
    let (_dir, mut store, epoch) = fresh();
    store.admission_budget.max_pending_effects = 1;
    let mut first = request(epoch, 1);
    first.effects.push(super::common::new_effect(
        "fx1",
        1,
        crate::EffectState::Pending,
    ));
    let mut second = request(epoch, 2);
    second.effects.push(super::common::new_effect(
        "fx2",
        1,
        crate::EffectState::Pending,
    ));
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&first).unwrap();
    assert!(matches!(
        group.append(&second),
        Err(StoreError::PendingEffectCapacity { pending: 1, .. })
    ));
    group.append(&request(epoch, 2)).unwrap();
    group.finish().unwrap();
    assert!(store.effect("fx1").unwrap().is_some());
    assert!(store.effect("fx2").unwrap().is_none());
}

#[test]
fn outer_commit_failure_rolls_back_every_logical_entry() {
    let (_dir, mut store, epoch) = fresh();
    store.conn.execute_batch("CREATE TABLE group_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE group_child(parent INTEGER REFERENCES group_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER group_deferred_failure AFTER INSERT ON commands BEGIN INSERT INTO group_child VALUES(999); END;").unwrap();
    let before = store.commit_statistics();
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&request(epoch, 1)).unwrap();
    group.append(&request(epoch, 2)).unwrap();
    assert!(matches!(group.finish(), Err(StoreError::Sqlite(_))));
    assert_eq!(store.command("c1").unwrap(), None);
    assert_eq!(store.command("c2").unwrap(), None);
    assert_eq!(store.current_cut().unwrap().last_batch, None);
    let after = store.commit_statistics();
    assert_eq!(after.physical_commits, before.physical_commits);
    assert_eq!(after.failed_commits, before.failed_commits + 1);
}

#[test]
fn internal_byte_boundary_splits_without_rejecting_a_valid_request() {
    let (_dir, mut store, epoch) = fresh();
    let mut first = request(epoch, 1);
    first.appends[0].events[0].payload.bytes = vec![0; 33 * 1024 * 1024];
    let mut second = request(epoch, 2);
    second.appends[0].events[0].payload.bytes = vec![1; 33 * 1024 * 1024];
    let mut group = store.begin_commit_group(epoch).unwrap();
    assert!(group.fits(&first).unwrap());
    group.append(&first).unwrap();
    assert!(!group.fits(&second).unwrap());
    group.finish().unwrap();
    let mut next = store.begin_commit_group(epoch).unwrap();
    assert!(next.fits(&second).unwrap());
    next.append(&second).unwrap();
    next.finish().unwrap();
    assert_eq!(store.stream_revision(&StreamId::new("a")).unwrap(), Some(2));
}
