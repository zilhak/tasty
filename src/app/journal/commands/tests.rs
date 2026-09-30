use super::*;
use std::time::{Duration, Instant};

fn request(method: &str, params: serde_json::Value, key: Option<&str>, id: u64) -> JsonRpcRequest {
    serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","method":method,"params":params,"id":id,"idempotency_key":key})).unwrap()
}

fn finish(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    receiver: &std::sync::mpsc::Receiver<JsonRpcResponse>,
) -> JsonRpcResponse {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        journal.poll_bootstrap(&mut [session]).unwrap();
        for (ticket, _) in journal.requests_needing_resolution() {
            journal.resolve_ipc_for_engine(ticket, session);
        }
        if let Ok(response) = receiver.try_recv() {
            return response;
        }
        assert!(Instant::now() < until, "structural IPC reply stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn send(
    journal: &mut JournalApplication,
    request: JsonRpcRequest,
) -> std::sync::mpsc::Receiver<JsonRpcResponse> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    assert!(journal.admit_ipc(
        &crate::ipc::server::IpcCommand::new(request, tx),
        &crate::ipc::caller::CallerContext::Local
    ));
    rx
}

#[test]
fn category_wire_results_and_rejections_survive_deletion_and_worker_restart() {
    let (mut session, mut journal) = boot();
    let until = Instant::now() + Duration::from_secs(10);
    let create = request(
        "workspace_category.create",
        serde_json::json!({"name":"  Work  "}),
        Some("category-create"),
        1,
    );
    let first_rx = send(&mut journal, create.clone());
    let mut duplicate = create.clone();
    duplicate.id = Some(serde_json::json!(2));
    let duplicate_rx = send(&mut journal, duplicate);
    let created = finish(&mut journal, &mut session, &first_rx);
    let duplicated = finish(&mut journal, &mut session, &duplicate_rx);
    assert_eq!(created.result, duplicated.result);
    assert!(duplicated.idempotent_replay);
    let category = created.result.as_ref().unwrap()["id"].as_u64().unwrap();
    assert_eq!(created.result.as_ref().unwrap()["name"], "Work");
    assert_eq!(
        session.core_state.categories.len(),
        2,
        "duplicate admission reserved and created once"
    );
    let rename = request(
        "workspace_category.rename",
        serde_json::json!({"id":category,"name":"  Renamed  "}),
        Some("rename"),
        3,
    );
    let rx = send(&mut journal, rename.clone());
    let renamed = finish(&mut journal, &mut session, &rx);
    assert_eq!(renamed.result.as_ref().unwrap()["name"], "  Renamed  ");
    assert_eq!(session.core_state.categories[1].name, "Renamed");
    let invalid = request(
        "workspace_category.create",
        serde_json::json!({"name":"renamed"}),
        Some("rejected-name"),
        4,
    );
    let rx = send(&mut journal, invalid.clone());
    let rejected = finish(&mut journal, &mut session, &rx);
    assert_eq!(rejected.error.as_ref().unwrap().code, -32602);
    let rx = send(
        &mut journal,
        request(
            "workspace_category.delete",
            serde_json::json!({"id":category}),
            Some("delete"),
            5,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    assert_eq!(session.core_state.categories.len(), 1);
    drop(journal);
    session.core_state.replace_local_workspaces(Vec::new());
    session.journal_binding = None;
    // The old TerminalStore owners are not a second structural source; bootstrap installs replacements.
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal
        .begin_engine(
            &session,
            EngineSelection::Slot {
                slot: 1,
                resume: true,
            },
        )
        .unwrap();
    while !journal.is_ready(session.id) {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    for (original, expected) in [(create, created), (rename, renamed), (invalid, rejected)] {
        let rx = send(&mut journal, original);
        let replay = finish(&mut journal, &mut session, &rx);
        assert_eq!(replay.result, expected.result);
        assert_eq!(
            serde_json::to_value(replay.error).unwrap(),
            serde_json::to_value(expected.error).unwrap()
        );
        assert!(replay.idempotent_replay);
    }
    assert_eq!(
        session.core_state.categories.len(),
        1,
        "retries neither resolve missing category nor recreate it"
    );
}

fn boot() -> (EngineSession, JournalApplication) {
    boot_with_layout(None)
}

fn boot_with_layout(layout: Option<serde_json::Value>) -> (EngineSession, JournalApplication) {
    let resume = layout.is_some();
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = "exec sleep 60".into();
    let mut session = EngineSession::for_journal(
        80,
        24,
        Arc::new(|| {}),
        None,
        Some(1),
        Arc::new(std::sync::Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        )),
        Arc::new(crate::core::agent::runner_thread::RunnerRegistry::new()),
        settings,
    )
    .unwrap();
    if let Some(layout) = layout {
        let directory = tasty_utils::path::tasty_home().unwrap().join("layouts");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("01.json"), layout.to_string()).unwrap();
    }
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal
        .begin_engine(&session, EngineSelection::Slot { slot: 1, resume })
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !journal.is_ready(session.id) {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        if resume {
            journal.poll_restore_bootstrap(&mut session).unwrap();
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    (session, journal)
}

#[test]
fn oversized_admission_and_resolution_do_not_halt_other_requests() {
    let (mut session, mut journal) = boot();
    let too_big = "x".repeat(crate::adapters::production::tcp_ipc_server::MAX_REQUEST_LINE_BYTES);
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":too_big}),
            Some("oversized"),
            1,
        ),
    );
    let response = finish(&mut journal, &mut session, &rx);
    assert_eq!(
        response.error.unwrap().code,
        crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG
    );
    assert!(!journal.is_halted());
    let large = "a".repeat(2 * 1024 * 1024);
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":large}),
            Some("large-valid"),
            2,
        ),
    );
    let created = finish(&mut journal, &mut session, &rx);
    assert!(created.error.is_none());
    assert_eq!(
        created.result.unwrap()["name"].as_str().unwrap().len(),
        large.len()
    );

    // A leader is already admitted and a follower joined before internal resolution is refused.
    let keyed = request(
        "workspace_category.create",
        serde_json::json!({"name":"small"}),
        Some("oversized-resolution"),
        3,
    );
    let leader = send(&mut journal, keyed.clone());
    let follower = send(&mut journal, keyed);
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        if journal
            .commands
            .pending
            .values()
            .any(|pending| pending.replay)
        {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    let ticket = *journal.commands.pending.keys().next().unwrap();
    journal
        .commands
        .pending
        .get_mut(&ticket)
        .unwrap()
        .needs_resolution = false;
    journal.commands.pending.get_mut(&ticket).unwrap().queued = Some(Work::Resolve {
        changes: Vec::new(),
        response: Some(ResponsePlan::Fixed(JsonRpcResponse::success(
            serde_json::Value::Null,
            serde_json::Value::String("z".repeat(tasty_ipc::admission::QUEUED_BYTES_LIMIT + 1)),
        ))),
    });
    let first = finish(&mut journal, &mut session, &leader);
    let second = finish(&mut journal, &mut session, &follower);
    assert_eq!(
        first.error.unwrap().code,
        crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG
    );
    assert_eq!(
        second.error.unwrap().code,
        crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG
    );
    assert!(journal.commands.pending.is_empty());
    assert!(!journal.is_halted());
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":"after-pressure"}),
            None,
            4,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
}

#[test]
fn resolved_rename_releases_raw_params_before_retaining_input_and_wire_reply() {
    let (mut session, mut journal) = boot();
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":"a"}),
            None,
            1,
        ),
    );
    let id = finish(&mut journal, &mut session, &rx).result.unwrap()["id"]
        .as_u64()
        .unwrap();
    let name = "b".repeat(2 * 1024 * 1024);
    let rx = send(
        &mut journal,
        request(
            "workspace_category.rename",
            serde_json::json!({"id":id,"name":name}),
            None,
            2,
        ),
    );
    let until = Instant::now() + Duration::from_secs(10);
    let ticket = loop {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        if let Some((ticket, _)) = journal.requests_needing_resolution().into_iter().next() {
            break ticket;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    };
    journal.resolve_ipc_for_engine(ticket, &session);
    let pending = &journal.commands.pending[&ticket];
    assert!(pending.request.params.is_null());
    let Work::Resolve { changes, response } = pending.queued.as_ref().unwrap() else {
        panic!("resolved");
    };
    let retained = serde_json::to_vec(&pending.request).unwrap().len()
        + serde_json::to_vec(&(changes, response)).unwrap().len();
    assert!(
        retained <= pending.bytes,
        "retained={retained} reserved={}",
        pending.bytes
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
}

#[test]
fn committed_publication_failure_halts_readers_and_fails_all_pending_replies() {
    let (mut session, mut journal) = boot();
    journal
        .worker
        .fail_next_publication
        .store(true, std::sync::atomic::Ordering::Release);
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":"committed-not-published"}),
            Some("halt-cut"),
            1,
        ),
    );
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if journal.poll_bootstrap(&mut [&mut session]).is_err() {
            break;
        }
        for (ticket, _) in journal.requests_needing_resolution() {
            journal.resolve_ipc_for_engine(ticket, &session);
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(journal.is_halted());
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .error
            .is_some()
    );
    assert!(journal.commands.pending.is_empty());
    assert!(
        !session
            .core_state
            .categories
            .iter()
            .any(|category| category.name == "committed-not-published")
    );
    assert!(
        journal
            .reject_halted_request(&request("tree", serde_json::json!({}), None, 2))
            .unwrap()
            .error
            .is_some()
    );
    let binding = session.journal_binding.clone().unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    drop(journal);
    let store = tasty_event_store::EventStore::open(
        &home.join("structure/journal.db"),
        &binding.journal_id,
    )
    .unwrap();
    let model =
        crate::runtime::journal::load(&store, &tasty_event_store::StreamId::new(&binding.stream))
            .unwrap();
    assert!(
        model
            .categories
            .values()
            .any(|category| category.name == "committed-not-published"),
        "failure is after durable commit, not an admission refusal"
    );
}

#[cfg(feature = "gui")]
#[test]
fn stale_settings_reset_completion_cannot_overwrite_a_newer_settings_intent() {
    let (mut session, mut journal) = boot();
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":"will-reset"}),
            None,
            8,
        ),
    );
    let category = finish(&mut journal, &mut session, &rx).result.unwrap()["id"]
        .as_u64()
        .unwrap() as u32;
    let workspace = session.core_state.local_workspaces[0].id;
    journal
        .admit_fixed_intent(
            &session,
            vec![tasty_domain::StructuralCommand::SetWorkspaceCategory {
                workspace_id: workspace,
                category,
            }],
            &crate::intent::IntentOrigin::System,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !journal.commands.pending.is_empty() {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(session.core_state.categories.len(), 2);
    assert_eq!(session.core_state.local_workspaces[0].category, category);
    let mut off = session.core_state.settings.clone();
    off.general.workspace_categories_enabled = false;
    let first = journal.note_settings_intent().unwrap();
    journal.admit_settings_reset(
        first,
        vec![StreamCommand {
            stream: session.journal_binding.as_ref().unwrap().stream.clone(),
            command: tasty_domain::StructuralCommand::ResetCategories,
        }],
        off,
        &crate::intent::IntentOrigin::System,
    );
    let second = journal.note_settings_intent().unwrap();
    assert!(second > first);
    // The ordinary service-side update B has been applied while A's reset awaits its commit.
    session
        .core_state
        .settings
        .general
        .workspace_categories_enabled = true;
    session.core_state.settings.general.startup_command = "latest-setting-B".into();
    let until = Instant::now() + Duration::from_secs(10);
    while !journal.commands.pending.is_empty() {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        journal.take_settings_results().is_empty(),
        "the actual App continuation drain must not deliver obsolete Settings A"
    );
    assert!(
        session
            .core_state
            .settings
            .general
            .workspace_categories_enabled
    );
    assert_eq!(
        session.core_state.settings.general.startup_command,
        "latest-setting-B"
    );
    assert_eq!(
        session.core_state.categories.len(),
        1,
        "committed reset facts are retained"
    );
    assert_eq!(session.core_state.local_workspaces[0].category, 0);
}

#[cfg(feature = "gui")]
#[test]
fn fixed_intent_budget_counts_its_commands_and_rejects_before_admission_without_losing_capacity() {
    let (mut session, mut journal) = boot();
    let tab = session.core_state.local_workspaces[0]
        .pane_layout()
        .first_pane()
        .unwrap()
        .tabs[0]
        .id;
    journal
        .admit_fixed_intent(
            &session,
            vec![tasty_domain::StructuralCommand::RenameTab {
                tab_id: tab,
                name: Some("fixed-name".into()),
            }],
            &crate::intent::IntentOrigin::System,
        )
        .unwrap();
    let ticket = *journal.commands.pending.keys().next().unwrap();
    let pending = &journal.commands.pending[&ticket];
    assert!(pending.fixed.is_some());
    assert!(pending.request.params.is_null());
    assert_eq!(pending.bytes, pending_weight(pending));
    assert!(!pending.admitted);
    // A larger derived fixed payload hits the pre-Admit stage, so no Resolve can be sent.
    journal.commands.pending.get_mut(&ticket).unwrap().fixed = Some(vec![StreamCommand {
        stream: session.journal_binding.as_ref().unwrap().stream.clone(),
        command: tasty_domain::StructuralCommand::RenameTab {
            tab_id: tab,
            name: Some("x".repeat(tasty_ipc::admission::QUEUED_BYTES_LIMIT)),
        },
    }]);
    journal.refresh_command_weight(ticket);
    assert!(journal.commands.pending.is_empty());
    let result = journal.commands.completed_intents.pop().unwrap();
    assert_eq!(
        result.response.error.unwrap().code,
        crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL
    );
    let rx = send(
        &mut journal,
        request(
            "workspace_category.create",
            serde_json::json!({"name":"after-fixed-pressure"}),
            None,
            1,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    assert!(!journal.is_halted());
}

#[cfg(not(feature = "gui"))]
#[test]
fn headless_pending_category_intent_uses_the_explicit_engine_journal_admission() {
    let (mut session, mut journal) = boot();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let mut state = crate::state::RequestContext::new(
        &mut session.borrow_mut(),
        core.preset_store.clone(),
        core.memory_arc(),
    );
    state.engine_id = Some(session.id);
    state.dispatch_intent(
        crate::core::intent::DomainIntent::CreateCategory {
            name: "headless-queued".into(),
        }
        .from_agent_ipc(),
    );
    crate::intent::headless::drain_pending_intents_in_app(
        &mut core,
        &mut state,
        &mut session.borrow_mut(),
        &mut journal,
    );
    assert_eq!(
        session.core_state.categories.len(),
        1,
        "draining the intent does not perform the old direct writer"
    );
    let pending = journal.commands.pending.values().next().unwrap();
    let Some(Work::Admit(header)) = &pending.queued else {
        panic!("journal admission");
    };
    assert_eq!(header.actor, "agent");
    let until = Instant::now() + Duration::from_secs(10);
    while !journal.commands.pending.is_empty() {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        journal.resolve_headless_requests(&session, &mut state);
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        session
            .core_state
            .categories
            .iter()
            .any(|category| category.name == "headless-queued")
    );
    assert!(!journal.is_halted());
}

#[path = "metadata_tests.rs"]
mod metadata;

#[test]
fn each_keyed_journal_request_reports_only_its_initial_retry_decision() {
    use crate::ipc::handler::idempotency::RetryOutcome as O;
    let (_session, mut journal) = boot();
    for (key, result, expected) in [
        (
            Some("new"),
            Ok(ResultValue::NeedsResolution),
            Some(O::Executed),
        ),
        (
            Some("joined"),
            Ok(ResultValue::JoinedAdmission { leader_ticket: 100 }),
            Some(O::InFlight),
        ),
        (
            Some("conflict"),
            Err("idempotency key was reused for a different request".into()),
            Some(O::Conflicted),
        ),
        (None, Ok(ResultValue::NeedsResolution), None),
    ] {
        let ticket = journal.next_ticket;
        let _receiver = send(
            &mut journal,
            request(
                "workspace_category.create",
                serde_json::json!({"name":"counter"}),
                key,
                ticket,
            ),
        );
        let pending = journal.commands.pending.get_mut(&ticket).unwrap();
        assert_eq!(pending.take_retry_outcome(&result), expected);
        assert_eq!(pending.take_retry_outcome(&result), None);
        assert_eq!(
            pending.take_retry_outcome(&Ok(ResultValue::Cancelled)),
            None
        );
    }
}

#[cfg(feature = "gui")]
#[path = "divider_tests.rs"]
mod divider;
