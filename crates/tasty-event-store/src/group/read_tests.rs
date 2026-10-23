use super::*;
use crate::{CommandKey, CommandLookup, CommandStatus, NewCommand};

fn request(epoch: WriterEpoch) -> CommitRequest {
    let mut request = CommitRequest::new(epoch);
    request.command = Some(NewCommand {
        command_id: "prefix".into(),
        key: Some(CommandKey {
            caller_scope: "read-fault".into(),
            idempotency_key: "prefix".into(),
        }),
        request_digest: vec![1],
        resolved: vec![],
        status: CommandStatus::Completed,
        response: Some(b"ok".to_vec()),
    });
    request
}

#[test]
fn every_private_read_sql_failure_prevents_commit_of_a_valid_prefix() {
    for reader in ["command", "lookup", "effect", "batch"] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = EventStore::open(&dir.path().join("journal.db"), "read-fault").unwrap();
        let epoch = store.acquire_writer().unwrap();
        let before = store.commit_statistics();
        let request = request(epoch);
        let mut group = store.begin_commit_group(epoch).unwrap();
        group.append(&request).unwrap();
        // Fault the real SQLite schema inside this private transaction, after a valid append.
        // Rollback must restore both the schema and the absence of the uncommitted prefix.
        let table = match reader {
            "command" | "lookup" => "commands",
            "effect" => "effects",
            _ => "batches",
        };
        group
            .tx
            .execute_batch(&format!("DROP TABLE {table}"))
            .unwrap();
        let error = match reader {
            "command" => group.command("prefix").unwrap_err(),
            "lookup" => group
                .lookup_command(
                    request.command.as_ref().unwrap().key.as_ref().unwrap(),
                    &[1],
                )
                .unwrap_err(),
            "effect" => group.effect("missing").unwrap_err(),
            _ => group.read_batch(1).unwrap_err(),
        };
        assert!(matches!(error, StoreError::Sqlite(_)), "{reader}: {error}");
        assert!(group.is_poisoned(), "{reader}");
        assert_reads_aborted(
            &group,
            request.command.as_ref().unwrap().key.as_ref().unwrap(),
        );
        assert!(matches!(
            group.append(&request),
            Err(StoreError::GroupAborted)
        ));
        assert!(matches!(group.finish(), Err(StoreError::GroupAborted)));
        assert!(store.command("prefix").unwrap().is_none(), "{reader}");
        assert_eq!(store.commit_statistics(), before);
    }
}

#[test]
fn missing_records_and_digest_mismatch_leave_the_valid_prefix_committable() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = EventStore::open(&dir.path().join("journal.db"), "read-control").unwrap();
    let epoch = store.acquire_writer().unwrap();
    let request = request(epoch);
    let mut group = store.begin_commit_group(epoch).unwrap();
    group.append(&request).unwrap();
    assert!(group.command("missing").unwrap().is_none());
    assert!(group.effect("missing").unwrap().is_none());
    let key = request.command.as_ref().unwrap().key.as_ref().unwrap();
    assert!(matches!(
        group.lookup_command(key, &[2]).unwrap(),
        CommandLookup::DigestMismatch(_)
    ));
    assert!(matches!(
        group
            .lookup_command(
                &CommandKey {
                    caller_scope: "missing".into(),
                    idempotency_key: "missing".into()
                },
                &[]
            )
            .unwrap(),
        CommandLookup::Miss
    ));
    assert!(matches!(
        group.read_batch(1),
        Err(StoreError::UnknownBatch(1))
    ));
    assert!(!group.is_poisoned());
    group.finish().unwrap();
    assert!(store.command("prefix").unwrap().is_some());
}

fn assert_reads_aborted(group: &CommitGroup<'_>, key: &CommandKey) {
    assert!(matches!(
        group.command("prefix"),
        Err(StoreError::GroupAborted)
    ));
    assert!(matches!(
        group.effect("missing"),
        Err(StoreError::GroupAborted)
    ));
    assert!(matches!(
        group.lookup_command(key, &[1]),
        Err(StoreError::GroupAborted)
    ));
    assert!(matches!(group.read_batch(1), Err(StoreError::GroupAborted)));
}
