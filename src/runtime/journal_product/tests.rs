use super::*;
use std::time::Duration;
use tasty_domain::{StructuralCommand, StructureModels, evolve_streams};

fn receive(worker: &JournalWorker) -> Completion {
    worker
        .completions
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
}

fn start(home: &std::path::Path) -> JournalWorker {
    let worker = JournalWorker::spawn(home.to_owned(), Arc::new(|| {})).unwrap();
    match receive(&worker) {
        Completion::Ready {
            cut,
            bootstrap,
            runtime_epoch,
            ..
        } => {
            assert!(runtime_epoch > 0);
            assert_eq!(cut, bootstrap.batch);
            worker.acknowledge(cut.unwrap_or(0), Ok(())).unwrap();
        }
        other => panic!("{other:?}"),
    }
    worker
}

fn submit(worker: &JournalWorker, ticket: u64, work: Work) {
    worker.submit(Request { ticket, work }).unwrap();
}

fn header(key: &str) -> Admission {
    Admission {
        key: Some(CommandKey {
            caller_scope: "test-agent".into(),
            idempotency_key: key.into(),
        }),
        original_digest: b"original method and params".to_vec(),
        actor: "agent".into(),
        origin: "ipc".into(),
        causation_id: None,
    }
}

fn finished(worker: &JournalWorker, ticket: u64) -> Result<ResultValue, String> {
    match receive(worker) {
        Completion::Finished {
            ticket: got,
            result,
        } => {
            assert_eq!(got, ticket);
            result
        }
        other => panic!("{other:?}"),
    }
}

fn category(stream: &str, id: u32) -> StreamCommand {
    StreamCommand {
        stream: format!("structure:{stream}"),
        command: StructuralCommand::CreateCategory {
            reserved_id: id,
            name: format!("category-{id}"),
        },
    }
}

#[test]
fn startup_barrier_backpressures_without_blocking_the_application_thread() {
    let home = tempfile::tempdir().unwrap();
    let worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    assert!(matches!(
        receive(&worker),
        Completion::Ready { cut: None, .. }
    ));
    for ticket in 0..QUEUE_CAPACITY as u64 {
        submit(&worker, ticket, Work::ReadEngine("structure:slot-1".into()));
    }
    assert_eq!(
        worker.submit(Request {
            ticket: 999,
            work: Work::CancelAdmission
        }),
        Err(SubmitError::Busy)
    );
    assert!(matches!(worker.try_recv(), Err(mpsc::TryRecvError::Empty)));
    worker.acknowledge(0, Ok(())).unwrap();
    for ticket in 0..QUEUE_CAPACITY as u64 {
        let ResultValue::Engine(model) = finished(&worker, ticket).unwrap() else {
            panic!("engine")
        };
        assert!(model.workspaces.is_empty());
    }
}

#[test]
fn duplicate_admission_resolves_once_and_multistream_result_waits_for_publication() {
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    submit(&worker, 1, Work::Admit(header("create")));
    assert!(matches!(
        finished(&worker, 1).unwrap(),
        ResultValue::NeedsResolution
    ));
    submit(&worker, 2, Work::Admit(header("create")));
    assert!(matches!(
        finished(&worker, 2).unwrap(),
        ResultValue::JoinedAdmission { leader_ticket: 1 }
    ));
    submit(&worker, 2, Work::Reserve(vec![(IdKind::Category, 2)]));
    assert!(finished(&worker, 2).unwrap_err().contains("admitted"));
    submit(&worker, 1, Work::Reserve(vec![(IdKind::Category, 2)]));
    let ResultValue::Reserved(ranges) = finished(&worker, 1).unwrap() else {
        panic!("reservation")
    };
    assert_eq!((ranges[0].start, ranges[0].end), (1, 3));
    submit(
        &worker,
        1,
        Work::Resolve(vec![category("slot-1", 1), category("slot-2", 2)]),
    );
    let Completion::Publish(batch) = receive(&worker) else {
        panic!("publication before response")
    };
    assert_eq!(batch.streams.len(), 2);
    assert!(matches!(worker.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let mut live = StructureModels::default();
    evolve_streams(&mut live, &batch).unwrap();
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    let ResultValue::Executed(first) = finished(&worker, 1).unwrap() else {
        panic!("execute")
    };
    let ResultValue::Executed(joined) = finished(&worker, 2).unwrap() else {
        panic!("joined")
    };
    assert_eq!(joined, first);
    assert_eq!(live.streams["structure:slot-1"].category_order, vec![1]);
    assert_eq!(live.streams["structure:slot-2"].category_order, vec![2]);
    submit(&worker, 3, Work::Admit(header("create")));
    let ResultValue::Stored(stored) = finished(&worker, 3).unwrap() else {
        panic!("stored")
    };
    assert_eq!(stored.command_id, first.command_id);
    assert_eq!(stored.batch_ids, vec![batch.batch_id]);
    drop(worker);
    let reopened = start(home.path());
    submit(&reopened, 4, Work::Admit(header("create")));
    assert!(matches!(
        finished(&reopened, 4).unwrap(),
        ResultValue::Stored(_)
    ));
}

#[test]
fn rejected_publication_halts_later_commands_without_returning_success() {
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    submit(&worker, 1, Work::Admit(header("create")));
    finished(&worker, 1).unwrap();
    submit(&worker, 1, Work::Reserve(vec![(IdKind::Category, 1)]));
    assert!(matches!(
        finished(&worker, 1).unwrap(),
        ResultValue::Reserved(_)
    ));
    submit(&worker, 1, Work::Resolve(vec![category("slot-1", 1)]));
    let Completion::Publish(batch) = receive(&worker) else {
        panic!("publish")
    };
    worker
        .acknowledge(batch.batch_id, Err("injected live apply failure".into()))
        .unwrap();
    assert!(finished(&worker, 1).unwrap_err().contains("halted"));
    submit(&worker, 2, Work::Admit(header("create")));
    assert!(finished(&worker, 2).unwrap_err().contains("halted"));
}

#[test]
fn shutdown_unblocks_a_worker_waiting_for_bootstrap_ack() {
    let home = tempfile::tempdir().unwrap();
    let worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    assert!(matches!(receive(&worker), Completion::Ready { .. }));
    drop(worker);
    // The previous owner released its writer lock and the explicit binding remains usable.
    drop(start(home.path()));
}

#[test]
fn an_active_binding_never_recreates_a_lost_database() {
    let home = tempfile::tempdir().unwrap();
    drop(start(home.path()));
    std::fs::remove_file(home.path().join("structure/journal.db")).unwrap();
    let worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    let Completion::StartupFailed(error) = receive(&worker) else {
        panic!("must refuse lost database")
    };
    assert!(error.contains("refusing to recreate"));
    assert!(!home.path().join("structure/journal.db").exists());
}

#[test]
fn interrupted_first_initialization_keeps_its_explicit_identity() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("structure")).unwrap();
    let id = "journal-0123456789abcdef0123456789abcdef";
    std::fs::write(
        home.path().join("structure/journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1, "journal_id": id, "phase": "Preparing"
        }))
        .unwrap(),
    )
    .unwrap();
    let worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    let Completion::Ready { journal_id, .. } = receive(&worker) else {
        panic!("ready")
    };
    assert_eq!(journal_id, id);
    worker.acknowledge(0, Ok(())).unwrap();
    let binding: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.path().join("structure/journal.json")).unwrap())
            .unwrap();
    assert_eq!(binding["phase"], "Ready");
}
