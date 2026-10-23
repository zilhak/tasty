//! Actual SQLite fault through the production completion builder and private executor staging.
use super::*;
use crate::runtime::command_executor::{ExecError, Executor, GroupCommand};

fn category(id: u32) -> ExecuteRequest<ResolvedCommand> {
    ExecuteRequest {
        key: None,
        actor: "test".into(),
        origin: "completion-read-test".into(),
        causation_id: None,
        command: ResolvedCommand {
            original_digest: vec![id as u8],
            response: None,
            changes: vec![StreamCommand {
                stream: "structure:slot-1".into(),
                command: StructuralCommand::CreateCategory {
                    reserved_id: id,
                    name: format!("category-{id}"),
                },
            }],
            effect_result: None,
            cancellation: None,
            completion_view: None,
            original_results: Default::default(),
        },
    }
}

fn run(read_fault: bool, swallow_error: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let store = EventStore::open(&path, "completion-read-test").unwrap();
    let executor = Executor::open(StructureDecider::default(), store).unwrap();
    if read_fault {
        rusqlite::Connection::open(&path).unwrap().execute_batch(
            "ALTER TABLE effects RENAME TO effects_real;
             CREATE VIEW effects AS SELECT * FROM effects_real WHERE completion_missing_function();"
        ).unwrap();
    }
    let before = executor.inner.lock().unwrap().store.commit_statistics();
    let result = executor.execute_group(2, |index, state, group| {
        if index == 0 {
            return Ok(GroupCommand::Ready(category(1)));
        }
        assert_eq!(state.streams["structure:slot-1"].categories.len(), 1);
        let lease = EffectLease {
            effect_id: "missing".into(),
            operation: tasty_core::OperationId("missing".into()),
            stream: "structure:slot-1".into(),
            runtime_epoch: group.writer_epoch().0,
            resource_generation: 1,
            attempt: 1,
        };
        // This is the same String -> Record boundary used by worker::group::Entry::build.
        let prepared = prepare(
            group,
            state,
            lease.clone(),
            StructuralCommand::FinishCleanup {
                operation: lease.operation,
            },
            "cleaned",
            None,
        )
        .map_err(ExecError::Record);
        assert!(prepared.is_err());
        assert_eq!(group.is_poisoned(), read_fault);
        if swallow_error {
            Ok(GroupCommand::Ready(category(2)))
        } else {
            prepared
        }
    });
    assert_eq!(result.results.len(), 2);
    assert_eq!(result.halted.is_some(), read_fault);
    let inner = executor.inner.lock().unwrap();
    let after = inner.store.commit_statistics();
    if read_fault {
        assert!(result.results.iter().all(|result| matches!(result, Err(ExecError::Store(error)) if matches!(error.as_ref(), tasty_event_store::StoreError::GroupAborted))));
        assert!(inner.state.streams.is_empty());
        assert!(inner.store.read_batches_after(None, 10).unwrap().is_empty());
        assert!(inner.store.command("cmd-1-1").unwrap().is_none());
        assert_eq!(after, before);
    } else {
        assert!(result.results[0].is_ok());
        assert!(matches!(result.results[1], Err(ExecError::Record(_))));
        assert_eq!(inner.state.streams["structure:slot-1"].categories.len(), 1);
        assert_eq!(inner.store.read_batches_after(None, 10).unwrap().len(), 1);
        assert_eq!(after.physical_commits - before.physical_commits, 1);
    }
    drop(inner);
    if read_fault {
        assert!(matches!(
            executor.execute(&category(2)),
            Err(ExecError::Halted)
        ));
    }
    drop(executor);
    let reopened = EventStore::open(&path, "completion-read-test").unwrap();
    assert_eq!(
        reopened.read_batches_after(None, 10).unwrap().len(),
        usize::from(!read_fault)
    );
}

#[test]
fn sqlite_read_failure_in_completion_discards_all_staged_responses_and_halts() {
    run(true, false);
}

#[test]
fn swallowed_sqlite_read_failure_still_discards_all_staged_responses_and_halts() {
    run(true, true);
}

#[test]
fn missing_effect_is_a_local_completion_error_and_preserves_the_valid_prefix() {
    run(false, false);
}
