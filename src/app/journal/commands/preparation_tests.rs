//! A terminal child starts on the spawner, so a held start neither blocks another engine nor
//! pauses observation, and every started child reaches an exact owner whatever arrives first.
use super::*;
use crate::runtime::effect_runner::{SPAWN_QUEUE_CAPACITY, spawn_test_job};

type Receiver = std::sync::mpsc::Receiver<JsonRpcResponse>;

/// Requests naming a workspace go to the engine that owns it; others go to the first engine.
fn turn(journal: &mut JournalApplication, sessions: &mut [&mut EngineSession]) {
    journal.poll_bootstrap(sessions, None).unwrap();
    let services = crate::ipc::handler::cli_entry_tests::test_core();
    for (ticket, request) in journal.requests_needing_resolution() {
        let index = sessions
            .iter()
            .position(|session| {
                session
                    .core_state
                    .local_workspaces()
                    .iter()
                    .any(|workspace| serde_json::json!(workspace.id) == request.params["id"])
            })
            .unwrap_or(0);
        let session = &mut *sessions[index];
        match request.method.as_str() {
            "surface.respawn_terminal" => {
                journal.resolve_public_creation(ticket, session, &services)
            }
            "workspace.create" => journal.resolve_workspace_creation(ticket, session, None),
            _ => journal.resolve_ipc_for_engine(ticket, session),
        }
    }
}

fn wait_until(
    journal: &mut JournalApplication,
    sessions: &mut [&mut EngineSession],
    what: &str,
    mut done: impl FnMut(&JournalApplication, &[&mut EngineSession]) -> bool,
) {
    let mut stall = StallBudget::new(journal);
    while !done(journal, sessions) {
        turn(journal, sessions);
        stall.nap(what);
    }
}

fn reply(
    journal: &mut JournalApplication,
    sessions: &mut [&mut EngineSession],
    rx: &Receiver,
) -> JsonRpcResponse {
    reply_labeled(journal, sessions, rx, "prepared creation")
}

fn reply_labeled(
    journal: &mut JournalApplication,
    sessions: &mut [&mut EngineSession],
    rx: &Receiver,
    what: &str,
) -> JsonRpcResponse {
    let mut stall = StallBudget::new(journal);
    loop {
        turn(journal, sessions);
        if let Ok(response) = rx.try_recv() {
            return response;
        }
        stall.nap(what);
    }
}

fn preparing(journal: &JournalApplication, engine: EngineId) -> bool {
    journal
        .creations
        .iter()
        .any(|((id, _), creation)| *id == engine && creation.is_preparing())
}

fn create(journal: &mut JournalApplication, id: u64) -> Receiver {
    send(
        journal,
        request("workspace.create", serde_json::json!({}), None, id),
    )
}

fn second_engine(first: &mut EngineSession, journal: &mut JournalApplication) -> EngineSession {
    let mut second = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(2),
            memory: Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            )),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        first.runtime.settings.clone(),
    )
    .unwrap();
    journal
        .begin_engine(
            &second,
            EngineSelection::Slot {
                slot: 2,
                resume: false,
            },
        )
        .unwrap();
    let id = second.id;
    wait_until(
        journal,
        &mut [first, &mut second],
        "second engine",
        |journal, _| journal.is_ready(id),
    );
    second
}

#[test]
fn a_held_terminal_start_lets_another_engine_finish_and_does_not_pause_observation() {
    let (mut first, mut journal) = boot();
    let mut second = second_engine(&mut first, &mut journal);
    let second_ws = second.core_state.local_workspaces()[0].id;
    let held = journal.spawner.hold();
    let executed = journal.spawner.executed();
    let engine = first.id;
    let created = create(&mut journal, 800);
    wait_until(
        &mut journal,
        &mut [&mut first, &mut second],
        "creation reaches the spawner",
        |journal, _| preparing(journal, engine),
    );
    assert!(!journal.pauses_observation());
    let independent = send(
        &mut journal,
        request(
            "workspace.update",
            serde_json::json!({"id": second_ws, "name": "independent"}),
            None,
            801,
        ),
    );
    let response = reply(&mut journal, &mut [&mut first, &mut second], &independent);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(second.core_state.local_workspaces()[0].name, "independent");
    assert!(matches!(
        created.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        journal.spawner.executed(),
        executed,
        "the start is still held"
    );
    drop(held);
    let response = reply(&mut journal, &mut [&mut first, &mut second], &created);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(journal.spawner.executed(), executed + 1);
}

#[test]
fn a_full_spawner_refuses_a_creation_before_anything_starts() {
    let (mut session, mut journal) = boot();
    let held = journal.spawner.hold();
    let executed = journal.spawner.executed();
    let lease = crate::runtime::journal_product::EffectLease {
        effect_id: "filler".into(),
        operation: tasty_core::OperationId("filler".into()),
        stream: "structure:slot-1".into(),
        runtime_epoch: 1,
        resource_generation: 1,
        attempt: 1,
    };
    let mut fillers = vec![
        journal
            .spawner
            .submit(spawn_test_job(lease.clone()))
            .unwrap_or_else(|(_, reason)| panic!("{reason}")),
    ];
    // Fill the queue only once the worker holds its job, so no slot frees up afterwards.
    let mut stall = StallBudget::new(&journal);
    while !journal.spawner.parked() {
        stall.nap("worker takes the first filler");
    }
    while let Ok(pending) = journal.spawner.submit(spawn_test_job(lease.clone())) {
        fillers.push(pending);
        assert!(
            fillers.len() <= SPAWN_QUEUE_CAPACITY + 1,
            "the queue is bounded"
        );
    }
    assert_eq!(fillers.len(), SPAWN_QUEUE_CAPACITY + 1);
    let refused = create(&mut journal, 810);
    let response = reply_labeled(&mut journal, &mut [&mut session], &refused, "refused");
    let error = response.error.expect("a full spawner refuses the creation");
    assert!(error.message.contains("queue is full"), "{error:?}");
    assert_ne!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED,
        "nothing started, so the refusal is a known failure"
    );
    assert!(!journal.pauses_observation());
    // Abandoned before they ran, the fillers never start a child.
    drop(fillers);
    drop(held);
    let next = create(&mut journal, 811);
    let response = reply_labeled(&mut journal, &mut [&mut session], &next, "next");
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(journal.spawner.executed(), executed + 1);
}

#[test]
fn a_target_that_changed_during_the_start_discards_the_new_child() {
    let (mut session, mut journal) = boot();
    let created = create(&mut journal, 820);
    assert!(
        reply(&mut journal, &mut [&mut session], &created)
            .error
            .is_none()
    );
    let surface = session
        .core_state
        .workspaces()
        .iter()
        .last()
        .unwrap()
        .all_surface_ids()[0];
    let activation = session
        .core_state
        .find_surface_by_id(surface)
        .and_then(|descriptor| descriptor.activation_generation);
    let held = journal.spawner.hold();
    let respawned = send(
        &mut journal,
        request(
            "surface.respawn_terminal",
            serde_json::json!({"surface_id": surface}),
            None,
            821,
        ),
    );
    let engine = session.id;
    wait_until(
        &mut journal,
        &mut [&mut session],
        "respawn reaches the spawner",
        |journal, _| preparing(journal, engine),
    );
    // Another owner replaced the target while the new child was starting.
    let replaced = session.runtime.terminals.replace(
        surface,
        tasty_terminal::Terminal::new_detached(80, 24),
        None,
    );
    let changed = session.runtime.terminals.generation(surface);
    if let Some((_, Some(pty))) = replaced {
        session
            .engine_release
            .get_or_insert_with(Default::default)
            .retain_pty(pty.retire());
    }
    drop(held);
    let response = reply(&mut journal, &mut [&mut session], &respawned);
    let error = response.error.expect("a stale child is not installed");
    assert!(error.message.contains("changed while"), "{error:?}");
    assert_eq!(session.runtime.terminals.generation(surface), changed);
    assert_eq!(
        session
            .core_state
            .find_surface_by_id(surface)
            .and_then(|descriptor| descriptor.activation_generation),
        activation
    );
    assert!(session.pending_materializations.is_empty());
    assert!(!journal.has_creation(session.id));
}

#[test]
fn a_released_engine_waits_for_the_child_that_was_still_starting() {
    let (mut session, mut journal) = boot();
    let held = journal.spawner.hold();
    let executed = journal.spawner.executed();
    let _created = create(&mut journal, 830);
    let engine = session.id;
    wait_until(
        &mut journal,
        &mut [&mut session],
        "creation reaches the spawner",
        |journal, _| preparing(journal, engine),
    );
    let mut release = crate::runtime::resource_retirement::EngineRelease::default();
    let keys: Vec<_> = journal.creations.keys().copied().collect();
    for key in keys {
        journal
            .creations
            .remove(&key)
            .unwrap()
            .retain_for_release(&mut release);
    }
    assert!(
        !release.poll(&mut session, None),
        "a job that has not reported is not a released child"
    );
    drop(held);
    let mut stall = StallBudget::new(&journal);
    while !release.poll(&mut session, None) {
        stall.nap("released child reap");
    }
    assert_eq!(
        journal.spawner.executed(),
        executed + 1,
        "the release took over a child that really started"
    );
}

/// A missing path would be replaced by the detected shell, so the configured shell is an existing
/// non-executable file named like a shell. It fails the executable check before any fork. The
/// returned guard keeps the file alive.
fn unexecutable_shell(session: &mut EngineSession) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let shell = dir.path().join("bash");
    std::fs::write(&shell, "").expect("non-executable shell file");
    session.runtime.settings.general.shell = shell.to_string_lossy().into_owned();
    dir
}

/// A misconfigured shell path fails before any child exists. The failure is known: the caller
/// gets an ordinary error, the same key replays that failure, and a corrected request starts.
#[test]
fn a_shell_that_cannot_start_is_a_known_failure() {
    let (mut session, mut journal) = boot();
    let configured = session.runtime.settings.general.shell.clone();
    let _shell = unexecutable_shell(&mut session);
    let created = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("missing-shell"),
            840,
        ),
    );
    let response = reply_labeled(&mut journal, &mut [&mut session], &created, "missing shell");
    let error = response
        .error
        .expect("a missing shell does not create a terminal");
    assert_ne!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED,
        "no child existed, so the failure is known: {error:?}"
    );
    #[cfg(unix)]
    assert!(error.message.contains("not executable"), "{error:?}");
    assert!(
        journal
            .creations
            .values()
            .all(|creation| creation.uncertain_reason().is_none()),
        "no uncertain creation remains"
    );
    let again = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("missing-shell"),
            841,
        ),
    );
    let replay = reply_labeled(
        &mut journal,
        &mut [&mut session],
        &again,
        "replayed failure",
    );
    let replayed = replay.error.expect("the recorded failure replays");
    assert_ne!(
        replayed.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED,
        "the recorded outcome is the known failure: {replayed:?}"
    );
    assert_eq!(session.core_state.local_workspaces().len(), 1);
    session.runtime.settings.general.shell = configured;
    let retried = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("corrected-shell"),
            842,
        ),
    );
    let response = reply_labeled(
        &mut journal,
        &mut [&mut session],
        &retried,
        "corrected shell",
    );
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(session.core_state.local_workspaces().len(), 2);
}

/// An engine's first workspace has no caller to answer. Its known failure would halt every
/// engine's journal, so it settles as an engine without a workspace like an unknown result.
#[test]
fn a_first_workspace_whose_shell_cannot_start_does_not_halt_the_journal() {
    let (mut first, mut journal) = boot();
    let configured = first.runtime.settings.general.shell.clone();
    let _shell = unexecutable_shell(&mut first);
    let second = second_engine(&mut first, &mut journal);
    assert!(!journal.is_halted());
    assert!(second.core_state.local_workspaces().is_empty());
    first.runtime.settings.general.shell = configured;
    let created = create(&mut journal, 850);
    let response = reply(&mut journal, &mut [&mut first], &created);
    assert!(response.error.is_none(), "{response:?}");
}
