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
    let Completion::Publish { batch, before } = receive(&worker) else {
        panic!("publication before response")
    };
    assert_eq!(batch.streams.len(), 2);
    assert!(
        before
            .values()
            .all(|model| model.applied.revision.is_none())
    );
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
    submit(
        &worker,
        1,
        Work::Reserve(vec![(IdKind::Category, 1), (IdKind::Surface, 1)]),
    );
    assert!(matches!(
        finished(&worker, 1).unwrap(),
        ResultValue::Reserved(_)
    ));
    submit(&worker, 1, Work::Resolve(vec![category("slot-1", 1)]));
    let Completion::Publish { batch, before } = receive(&worker) else {
        panic!("publish")
    };
    assert!(
        before
            .values()
            .all(|model| model.applied.revision.is_none())
    );
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

fn publish(worker: &JournalWorker) -> tasty_domain::StreamBatch {
    let Completion::Publish { batch, .. } = receive(worker) else {
        panic!("committed batch")
    };
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    batch
}

fn prepare_workspace(
    worker: &JournalWorker,
    ticket: u64,
    name: &str,
    kind: &str,
) -> tasty_domain::OperationId {
    use tasty_domain::{CreationDestination, CreationPlan, DataRef, OperationId, SurfaceSpec};
    submit(worker, ticket, Work::Admit(header(name)));
    assert!(matches!(
        finished(worker, ticket).unwrap(),
        ResultValue::NeedsResolution
    ));
    submit(
        worker,
        ticket,
        Work::Reserve(vec![
            (IdKind::Workspace, 1),
            (IdKind::Pane, 1),
            (IdKind::Tab, 1),
            (IdKind::Surface, 1),
        ]),
    );
    let ResultValue::Reserved(ranges) = finished(worker, ticket).unwrap() else {
        panic!("IDs")
    };
    let id = |kind: IdKind| {
        ranges
            .iter()
            .find(|range| range.kind == kind.label())
            .unwrap()
            .start as u32
    };
    submit(
        worker,
        ticket,
        Work::PutPreparation(PreparationInput {
            kind: kind.into(),
            cwd: None,
            params: serde_json::json!({}),
            shell: (kind == "terminal").then(|| ShellRecipe {
                executable: "/bin/sh".into(),
                arguments: vec![
                    "-c".into(),
                    "printf '\\033]2;prepared-title\\007PREPARED-JOURNAL'; exec sleep 60".into(),
                ],
                environment: Vec::new(),
                cols: 80,
                rows: 24,
                scrollback_lines: 100,
                disk_scrollback: false,
                startup_command: String::new(),
                restore_command: None,
            }),
            restore: None,
        }),
    );
    let ResultValue::InputStored(DataRef(input)) = finished(worker, ticket).unwrap() else {
        panic!("input")
    };
    submit(
        worker,
        ticket,
        Work::Resolve(vec![StreamCommand {
            stream: "structure:slot-1".into(),
            command: StructuralCommand::PrepareCreation {
                operation: OperationId(String::new()),
                command_id: String::new(),
                input: DataRef(input),
                plan: CreationPlan {
                    destination: CreationDestination::Workspace {
                        workspace: id(IdKind::Workspace),
                        pane: id(IdKind::Pane),
                        tab: id(IdKind::Tab),
                        name: name.into(),
                        category: 1,
                        subtitle: String::new(),
                        description: String::new(),
                    },
                    surface: SurfaceSpec {
                        id: id(IdKind::Surface),
                        kind: kind.into(),
                        data: None,
                    },
                    tab_name: "created".into(),
                    explicit_name: None,
                },
            },
        }]),
    );
    let batch = publish(worker);
    let operation = batch.streams["structure:slot-1"]
        .iter()
        .find_map(|recorded| match &recorded.event {
            tasty_domain::DomainEvent::OperationPrepared { operation } => {
                Some(operation.id.clone())
            }
            _ => None,
        })
        .unwrap();
    let ResultValue::Executed(executed) = finished(worker, ticket).unwrap() else {
        panic!("prepare commit")
    };
    assert_eq!(
        executed.status,
        tasty_event_store::CommandStatus::InProgress
    );
    assert_eq!(executed.response, None);
    operation
}

fn seed_category(worker: &JournalWorker) {
    submit(worker, 100, Work::Admit(header("category")));
    finished(worker, 100).unwrap();
    submit(
        worker,
        100,
        Work::Reserve(vec![(IdKind::Category, 1), (IdKind::Surface, 1)]),
    );
    finished(worker, 100).unwrap();
    submit(worker, 100, Work::Resolve(vec![category("slot-1", 1)]));
    publish(worker);
    finished(worker, 100).unwrap();
}

fn claim(
    worker: &JournalWorker,
    ticket: u64,
    operation: tasty_domain::OperationId,
) -> ClaimedPreparation {
    submit(
        worker,
        ticket,
        Work::ClaimPreparation {
            stream: "structure:slot-1".into(),
            operation,
        },
    );
    let ResultValue::Claimed(claimed) = finished(worker, ticket).unwrap() else {
        panic!("claim")
    };
    claimed
}

#[test]
fn mixed_operation_leases_are_rejected_before_either_effect_or_model_changes() {
    use tasty_domain::{OperationId, PreparationResult};
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    seed_category(&worker);
    let first = prepare_workspace(&worker, 1, "first", "empty");
    let second = prepare_workspace(&worker, 2, "second", "empty");
    let first = claim(&worker, 11, first);
    let second = claim(&worker, 12, second);
    assert_eq!(first.lease.attempt, second.lease.attempt);
    assert_eq!(
        first.lease.resource_generation,
        second.lease.resource_generation
    );
    let mut mixed = first.lease.clone();
    mixed.operation = second.lease.operation.clone();
    submit(
        &worker,
        20,
        Work::Prepared {
            lease: mixed,
            result: PreparationResult::Ready { data: None },
        },
    );
    assert!(
        finished(&worker, 20)
            .unwrap_err()
            .contains("does not belong")
    );
    submit(&worker, 21, Work::ReadEngine("structure:slot-1".into()));
    let ResultValue::Engine(model) = finished(&worker, 21).unwrap() else {
        panic!("model")
    };
    assert!(model.workspaces.is_empty());
    for operation in [&first.lease.operation, &second.lease.operation] {
        assert_eq!(model.operations[operation].outcome, None);
    }
    // Each original receipt remains usable; neither lease was consumed by the mixed attempt.
    for (ticket, claimed) in [(30, first), (31, second)] {
        submit(
            &worker,
            ticket,
            Work::Prepared {
                lease: claimed.lease,
                result: PreparationResult::Failed {
                    reason: "fixture declined preparation".into(),
                },
            },
        );
        publish(&worker);
        finished(&worker, ticket).unwrap();
    }
    submit(
        &worker,
        32,
        Work::CleanupFinished {
            lease: EffectLease {
                effect_id: "missing".into(),
                operation: OperationId("missing".into()),
                stream: "structure:slot-1".into(),
                runtime_epoch: 1,
                resource_generation: 1,
                attempt: 1,
            },
        },
    );
    assert!(finished(&worker, 32).is_err());
}

#[cfg(unix)]
#[test]
fn a_durable_claim_precedes_real_pty_preparation_and_the_candidate_stays_private() {
    use crate::runtime::effect_runner::{self, ExecutionBinding};
    use tasty_domain::PreparationResult;
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    seed_category(&worker);
    let operation = prepare_workspace(&worker, 1, "terminal", "terminal");
    let claimed = claim(&worker, 2, operation);
    let binding = ExecutionBinding {
        stream: claimed.lease.stream.clone(),
        runtime_epoch: claimed.lease.runtime_epoch,
        engine_incarnation: claimed.engine_incarnation,
    };
    let (_view, mut session) = crate::state::tests::test_state();
    let mut engine = session.borrow_mut();
    let old_ids = engine.live_surface_ids();
    let reserved_surface = claimed.plan.surface.id;
    let prepared = effect_runner::prepare(&mut engine, &binding, claimed).unwrap();
    assert_eq!(
        engine.live_surface_ids(),
        old_ids,
        "preparing does not insert a candidate into the live tree"
    );
    assert!(prepared.publication.is_none());
    assert_eq!(prepared.leaf.surface.surface_id(), Some(reserved_surface));
    let (terminal, pty) = prepared.connection.as_ref().unwrap();
    assert_eq!(terminal.resource_generation(), pty.generation());
    assert!(pty.process_id().is_some());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !terminal
        .with_content(|reader| reader.screen_text(false))
        .contains("PREPARED-JOURNAL")
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let lease = prepared.lease.clone();
    let (_, candidate) = prepared.connection.unwrap();
    let retired = candidate.retire();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while retired.observation().phase != tasty_terminal::PtyPhase::Reaped {
        assert_ne!(
            retired.observation().phase,
            tasty_terminal::PtyPhase::WaitFailed
        );
        assert!(
            std::time::Instant::now() < deadline,
            "candidate was not reaped"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    submit(
        &worker,
        3,
        Work::Prepared {
            lease,
            result: PreparationResult::Failed {
                reason: "candidate deliberately discarded".into(),
            },
        },
    );
    publish(&worker);
    finished(&worker, 3).unwrap();
    assert_eq!(engine.live_surface_ids(), old_ids);
}

#[cfg(unix)]
#[test]
fn committed_installation_preserves_initial_observations_and_defers_command_completion() {
    use crate::runtime::{effect_runner, live_projection};
    use tasty_domain::{DomainBatch, PreparationResult};
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    seed_category(&worker);
    let operation = prepare_workspace(&worker, 1, "install", "terminal");
    let claimed = claim(&worker, 2, operation);
    let sid = claimed.plan.surface.id;
    let binding = effect_runner::ExecutionBinding {
        stream: claimed.lease.stream.clone(),
        runtime_epoch: claimed.lease.runtime_epoch,
        engine_incarnation: claimed.engine_incarnation,
    };
    let (_view, mut session) = crate::state::tests::test_state();
    let mut engine = session.borrow_mut();
    // The worker owns the only canonical predecessor; this fixture projects its category.
    // Its unrelated original Terminal stays in the store to catch accidental SID replacement.
    engine.replace_local_workspaces(Vec::new());
    engine.categories = vec![crate::model::WorkspaceCategory::new(1, "category-1".into())];
    let prepared = effect_runner::prepare(&mut engine, &binding, claimed).unwrap();
    let generation = prepared
        .connection
        .as_ref()
        .unwrap()
        .0
        .resource_generation();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !prepared
        .connection
        .as_ref()
        .unwrap()
        .0
        .with_content(|view| view.screen_text(false))
        .contains("PREPARED-JOURNAL")
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let lease = prepared.lease.clone();
    submit(
        &worker,
        3,
        Work::Prepared {
            lease,
            result: PreparationResult::Ready { data: None },
        },
    );
    let Completion::Publish { batch, before } = receive(&worker) else {
        panic!("publication")
    };
    let (leaf, installation) = prepared.into_installation();
    let mut leaves = std::collections::HashMap::from([(sid, leaf)]);
    let mut retired = Vec::new();
    live_projection::apply(
        engine.core,
        &before[&binding.stream],
        &DomainBatch {
            batch_id: batch.batch_id,
            events: batch.streams[&binding.stream].clone(),
        },
        &mut leaves,
        &mut retired,
    )
    .unwrap();
    let installed: effect_runner::Installed =
        installation.install(&mut engine, None, None).unwrap();
    assert_eq!(engine.runtime.terminals.generation(sid), Some(generation));
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    finished(&worker, 3).unwrap();
    submit(&worker, 4, Work::Admit(header("install")));
    let ResultValue::Stored(record) = finished(&worker, 4).unwrap() else {
        panic!("stored")
    };
    assert_eq!(record.status, tasty_event_store::CommandStatus::InProgress);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !installed.cleanup_complete().unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    submit(
        &worker,
        5,
        Work::CleanupFinished {
            lease: installed.lease,
        },
    );
    let Completion::Publish { batch, before } = receive(&worker) else {
        panic!("completion publication")
    };
    live_projection::apply(
        engine.core,
        &before[&binding.stream],
        &DomainBatch {
            batch_id: batch.batch_id,
            events: batch.streams[&binding.stream].clone(),
        },
        &mut leaves,
        &mut retired,
    )
    .unwrap();
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    finished(&worker, 5).unwrap();
    submit(&worker, 6, Work::Admit(header("install")));
    let ResultValue::Stored(record) = finished(&worker, 6).unwrap() else {
        panic!("stored")
    };
    assert_eq!(record.status, tasty_event_store::CommandStatus::Completed);
    assert!(record.response.is_some());
    assert!(
        engine
            .runtime
            .terminals
            .get(sid)
            .unwrap()
            .with_content(|view| view.screen_text(false))
            .contains("PREPARED-JOURNAL")
    );
    let events = engine.collect_events();
    assert!(events.iter().any(|event| event.surface_id == sid && event.generation == generation && matches!(&event.kind, tasty_terminal::TerminalEventKind::TitleChanged(title) if title == "prepared-title")));
    complete_conversion_and_reap(&worker, &mut engine, &binding, sid);
}

#[test]
fn kind_withdrawal_after_claim_prevents_factory_execution() {
    use crate::runtime::effect_runner::{self, ExecutionBinding};
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    seed_category(&worker);
    let operation = prepare_workspace(&worker, 1, "withdrawn", "late-kind");
    let claimed = claim(&worker, 2, operation);
    let binding = ExecutionBinding {
        stream: claimed.lease.stream.clone(),
        runtime_epoch: claimed.lease.runtime_epoch,
        engine_incarnation: claimed.engine_incarnation,
    };
    let (_view, mut session) = crate::state::tests::test_state();
    let mut engine = session.borrow_mut();
    let (sender, receiver) = std::sync::mpsc::channel();
    let declaration = serde_json::from_value(serde_json::json!({"kind":"late-kind", "display_name_i18n_key":"surface.kind.markdown", "rendering":"remote"})).unwrap();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.surface_registry,
        "com.test.late",
        &declaration,
        sender,
    );
    engine.surface_registry.withdraw_plugin("com.test.late");
    assert!(engine.surface_registry.get("late-kind").is_some());
    let error = match effect_runner::prepare(&mut engine, &binding, claimed) {
        Ok(_) => panic!("withdrawn kind prepared"),
        Err(error) => error,
    };
    assert!(
        error
            .downcast_ref::<crate::core::surface_registry::SurfaceKindWithdrawn>()
            .is_some()
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg(unix)]
fn complete_conversion_and_reap(
    worker: &JournalWorker,
    engine: &mut crate::core::engine_access::EngineMut<'_>,
    binding: &crate::runtime::effect_runner::ExecutionBinding,
    sid: u32,
) {
    use crate::runtime::{effect_runner, live_projection};
    use tasty_domain::{
        CreationDestination, CreationPlan, DomainBatch, DomainEvent, OperationId,
        PreparationResult, SurfaceSpec,
    };
    let original = engine.runtime.terminals.generation(sid).unwrap();
    let original_pid = engine
        .runtime
        .terminals
        .pty(sid)
        .unwrap()
        .process_id()
        .unwrap();
    submit(worker, 10, Work::Admit(header("convert")));
    finished(worker, 10).unwrap();
    submit(
        worker,
        10,
        Work::PutPreparation(PreparationInput {
            kind: "empty".into(),
            cwd: None,
            params: serde_json::json!({}),
            shell: None,
            restore: None,
        }),
    );
    let ResultValue::InputStored(input) = finished(worker, 10).unwrap() else {
        panic!("input")
    };
    submit(
        worker,
        10,
        Work::Resolve(vec![StreamCommand {
            stream: binding.stream.clone(),
            command: StructuralCommand::PrepareCreation {
                operation: OperationId(String::new()),
                command_id: String::new(),
                input,
                plan: CreationPlan {
                    destination: CreationDestination::Convert {
                        surface: sid,
                        previous_activation: Some(1),
                        explicit_name: Some(None),
                    },
                    surface: SurfaceSpec {
                        id: sid,
                        kind: "empty".into(),
                        data: None,
                    },
                    tab_name: String::new(),
                    explicit_name: None,
                },
            },
        }]),
    );
    let batch = publish(worker);
    let operation = batch.streams[&binding.stream]
        .iter()
        .find_map(|event| match &event.event {
            DomainEvent::OperationPrepared { operation } => Some(operation.id.clone()),
            _ => None,
        })
        .unwrap();
    finished(worker, 10).unwrap();
    let claimed = claim(worker, 11, operation);
    let candidate = effect_runner::prepare(engine, binding, claimed).unwrap();
    assert_eq!(engine.runtime.terminals.generation(sid), Some(original));
    assert_eq!(
        engine.runtime.terminals.pty(sid).unwrap().process_id(),
        Some(original_pid)
    );
    let lease = candidate.lease.clone();
    submit(
        worker,
        12,
        Work::Prepared {
            lease,
            result: PreparationResult::Ready { data: None },
        },
    );
    let Completion::Publish { batch, before } = receive(worker) else {
        panic!("publication")
    };
    let (leaf, installation) = candidate.into_installation();
    let mut leaves = std::collections::HashMap::from([(sid, leaf)]);
    let mut retired = Vec::new();
    live_projection::apply(
        engine.core,
        &before[&binding.stream],
        &DomainBatch {
            batch_id: batch.batch_id,
            events: batch.streams[&binding.stream].clone(),
        },
        &mut leaves,
        &mut retired,
    )
    .unwrap();
    let Some(live_projection::Retired::Surface(old)) = retired.first() else {
        panic!("retired surface")
    };
    let installed = installation
        .install(engine, None, Some(old.as_ref()))
        .unwrap();
    assert!(engine.runtime.terminals.get(sid).is_none());
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    finished(worker, 12).unwrap();
    submit(worker, 13, Work::Admit(header("convert")));
    let ResultValue::Stored(record) = finished(worker, 13).unwrap() else {
        panic!("stored")
    };
    assert_eq!(record.status, tasty_event_store::CommandStatus::InProgress);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !installed.cleanup_complete().unwrap() {
        assert!(
            std::time::Instant::now() < deadline,
            "exact original child not reaped"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{original_pid}")).exists());
    submit(
        worker,
        14,
        Work::CleanupFinished {
            lease: installed.lease,
        },
    );
    let Completion::Publish { batch, before } = receive(worker) else {
        panic!("completion")
    };
    live_projection::apply(
        engine.core,
        &before[&binding.stream],
        &DomainBatch {
            batch_id: batch.batch_id,
            events: batch.streams[&binding.stream].clone(),
        },
        &mut leaves,
        &mut retired,
    )
    .unwrap();
    worker.acknowledge(batch.batch_id, Ok(())).unwrap();
    finished(worker, 14).unwrap();
    submit(worker, 15, Work::Admit(header("convert")));
    let ResultValue::Stored(record) = finished(worker, 15).unwrap() else {
        panic!("stored")
    };
    assert_eq!(record.status, tasty_event_store::CommandStatus::Completed);
    assert_eq!(engine.find_surface_by_id(sid).unwrap().kind(), "empty");
}
