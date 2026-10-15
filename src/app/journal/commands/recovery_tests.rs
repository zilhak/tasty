//! A creation whose result is unknown answers its caller as recovery-required, does not hold
//! observation, and is not polled again without new evidence.
use super::*;

type Receiver = std::sync::mpsc::Receiver<JsonRpcResponse>;

fn turn(journal: &mut JournalApplication, session: &mut EngineSession) {
    journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
    let services = crate::ipc::handler::cli_entry_tests::test_core();
    for (ticket, request) in journal.requests_needing_resolution() {
        match request.method.as_str() {
            "workspace.create" => journal.resolve_workspace_creation(ticket, session, None),
            "surface.respawn_terminal" => {
                journal.resolve_public_creation(ticket, session, &services)
            }
            _ => journal.resolve_ipc_for_engine(ticket, session),
        }
    }
}

fn reply(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    rx: &Receiver,
    what: &str,
) -> JsonRpcResponse {
    let mut stall = StallBudget::new(journal);
    loop {
        turn(journal, session);
        if let Ok(response) = rx.try_recv() {
            return response;
        }
        stall.nap_with(|| {
            let reasons: Vec<_> = journal
                .creations
                .values()
                .map(|creation| creation.uncertain_reason().map(str::to_owned))
                .collect();
            format!("{what}: creations {reasons:?}")
        });
    }
}

/// The shell cannot start, and the failure is reported after the child may have been forked.
fn unstartable_shell(session: &mut EngineSession) {
    session.runtime.settings.general.shell = "/nonexistent/tasty-test-shell".into();
}

#[test]
fn an_unknown_creation_answers_recovery_required_without_holding_or_polling() {
    let (mut session, mut journal) = boot();
    unstartable_shell(&mut session);
    let rx = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("unknown-creation"),
            930,
        ),
    );
    let response = reply(&mut journal, &mut session, &rx, "unknown creation reply");
    let error = response.error.expect("an unknown result is not a success");
    assert_eq!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED,
        "{error:?}"
    );
    assert!(!journal.pauses_observation());
    // The same key reads the recorded outcome and starts nothing again.
    let again = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("unknown-creation"),
            931,
        ),
    );
    let replay = reply(
        &mut journal,
        &mut session,
        &again,
        "replayed unknown creation",
    );
    assert_eq!(
        replay.error.map(|error| error.code),
        Some(crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED)
    );
    let mut stall = StallBudget::new(&journal);
    while journal.cleanup_poll_deadline().is_some() {
        turn(&mut journal, &mut session);
        stall.nap("uncertain creation stops polling without evidence");
    }
    assert_eq!(session.core_state.local_workspaces().len(), 1);
}

/// The App polls the journal only when woken or at its cleanup deadline. The recovery-required
/// reply must not wait for an unrelated event to wake it.
#[test]
fn an_unknown_creation_is_answered_without_an_outside_wake() {
    let woken = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = woken.clone();
    let (mut session, mut journal) = boot_with(
        None,
        Arc::new(move || flag.store(true, std::sync::atomic::Ordering::Release)),
    );
    unstartable_shell(&mut session);
    let rx = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({}),
            Some("unknown-creation-wake"),
            940,
        ),
    );
    let mut stall = StallBudget::new(&journal);
    let response = loop {
        let due = journal
            .cleanup_poll_deadline()
            .is_some_and(|deadline| deadline <= std::time::Instant::now());
        if woken.swap(false, std::sync::atomic::Ordering::AcqRel) || due {
            turn(&mut journal, &mut session);
        }
        if let Ok(response) = rx.try_recv() {
            break response;
        }
        stall.nap("unknown creation reply driven only by wakes and deadlines");
    };
    assert_eq!(
        response.error.map(|error| error.code),
        Some(crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED)
    );
}
