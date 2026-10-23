//! A child's structure is published before its one-shot CR. Only the original reply waits for
//! the input; a target that changes after publication never receives the late CR.
use super::*;

type Receiver = std::sync::mpsc::Receiver<JsonRpcResponse>;

/// The command prints a value that its typed echo does not contain.
const COMMAND: &str = "echo $((6*7))";
const OUTPUT: &str = "42";

fn turn(journal: &mut JournalApplication, session: &mut EngineSession) {
    journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
    for (ticket, request) in journal.requests_needing_resolution() {
        match request.method.as_str() {
            "terminal.spawn" => journal.resolve_public_creation(ticket, session),
            "workspace.create" => journal.resolve_workspace_creation(ticket, session, None),
            _ => journal.resolve_ipc_for_engine(ticket, session),
        }
    }
}

/// The quiet test shell replaces itself with `sleep`; a child that runs commands needs a shell.
fn boot_with_shell() -> (EngineSession, JournalApplication) {
    let (mut session, journal) = boot();
    session.runtime.settings.general.startup_command = String::new();
    (session, journal)
}

fn spawn(journal: &mut JournalApplication, session: &EngineSession, id: u64) -> Receiver {
    let workspace = &session.core_state.local_workspaces()[0];
    send(
        journal,
        request(
            "terminal.spawn",
            serde_json::json!({
                "parent": workspace.all_surface_ids()[0],
                "workspace": workspace.id.to_string(),
                "command": COMMAND,
            }),
            Some("child-input-key"),
            id,
        ),
    )
}

fn child_surface(session: &EngineSession) -> Option<u32> {
    let workspace = &session.core_state.local_workspaces()[0];
    let parent = workspace.all_surface_ids()[0];
    workspace
        .all_surface_ids()
        .into_iter()
        .find(|surface| *surface != parent)
}

fn waiting_input(journal: &JournalApplication) -> bool {
    journal
        .creations
        .values()
        .any(|creation| creation.waits_child_input())
}

fn outputs(session: &EngineSession, surface: u32) -> usize {
    session
        .runtime
        .terminals
        .get(surface)
        .map(|terminal| terminal.with_content(|view| view.screen_text(false)))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.trim() == OUTPUT)
        .count()
}

/// Pumps until the child is published while its CR is still pending.
fn published_before_submit(journal: &mut JournalApplication, session: &mut EngineSession) -> u32 {
    let mut stall = StallBudget::new(journal);
    loop {
        turn(journal, session);
        if let Some(child) = child_surface(session)
            && waiting_input(journal)
        {
            assert!(
                !journal.pauses_observation(),
                "a pending child CR does not pause observation"
            );
            return child;
        }
        stall.nap("child published before its CR");
    }
}

fn reply(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    rx: &Receiver,
) -> JsonRpcResponse {
    let mut stall = StallBudget::new(journal);
    loop {
        turn(journal, session);
        if let Ok(response) = rx.try_recv() {
            return response;
        }
        stall.nap("child spawn reply");
    }
}

#[test]
fn a_child_is_published_before_its_command_runs_and_the_reply_waits_for_the_submit() {
    let (mut session, mut journal) = boot_with_shell();
    let parent = session.core_state.local_workspaces()[0].all_surface_ids()[0];
    // Consume an actual reservation so the child's real name cannot be mistaken for index zero.
    assert_eq!(
        session
            .runtime
            .child_terminals
            .reserve_index(&parent.into()),
        Ok(0)
    );
    let rx = spawn(&mut journal, &session, 900);
    let child = published_before_submit(&mut journal, &mut session);
    let pane = session.core_state.find_pane_for_surface(child).unwrap();
    let tab = session.core_state.find_tab_for_surface(child).unwrap();
    let tab = session
        .core_state
        .find_pane_by_id(pane)
        .unwrap()
        .tabs
        .iter()
        .find(|candidate| candidate.id == tab)
        .unwrap();
    assert_eq!(tab.name, "child1");
    assert_eq!(tab.explicit_name.as_deref(), Some("child1"));
    assert!(
        matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
        "the reply waits for the submit"
    );
    let response = reply(&mut journal, &mut session, &rx);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(response.result.as_ref().unwrap()["child_index"], 1);
    assert!(!waiting_input(&journal));
    let terminal = session
        .runtime
        .terminals
        .pty(child)
        .and_then(|pty| pty.process_id());
    let mut stall = StallBudget::for_process(terminal);
    while outputs(&session, child) == 0 {
        stall.nap_with(|| {
            format!(
                "child command output: {:?}",
                session
                    .runtime
                    .terminals
                    .get(child)
                    .map(|terminal| terminal.with_content(|view| view.screen_text(false)))
            )
        });
    }
    // The same key replays the stored result without writing the body again.
    let again = spawn(&mut journal, &session, 901);
    let replay = reply(&mut journal, &mut session, &again);
    assert_eq!(replay.result, response.result);
    for _ in 0..50 {
        turn(&mut journal, &mut session);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(outputs(&session, child), 1, "body and CR were sent once");
}

#[test]
fn a_child_held_after_publication_never_receives_the_late_submit() {
    let (mut session, mut journal) = boot_with_shell();
    let rx = spawn(&mut journal, &session, 910);
    let child = published_before_submit(&mut journal, &mut session);
    session
        .borrow_mut()
        .live
        .occupancy
        .acquire(child, 99)
        .unwrap();
    let response = reply(&mut journal, &mut session, &rx);
    let error = response
        .error
        .expect("an undelivered submit is not a success");
    assert_eq!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED
    );
    assert!(
        error.message.contains("published its structure"),
        "{error:?}"
    );
    assert!(error.message.contains("one-shot input"), "{error:?}");
    assert_eq!(child_surface(&session), Some(child), "the structure stays");
    for _ in 0..50 {
        turn(&mut journal, &mut session);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(outputs(&session, child), 0, "the late CR was not sent");
    assert!(!journal.has_creation(session.id));
}

#[test]
fn a_held_relationship_receipt_keeps_command_and_original_reply_pending_after_publication() {
    let (mut session, mut journal) = boot_with_shell();
    let rx = spawn(&mut journal, &session, 902);
    let mut stall = StallBudget::new(&journal);
    loop {
        turn(&mut journal, &mut session);
        if journal
            .commands
            .pending
            .values()
            .any(|pending| pending.child_spawn.is_some())
        {
            break;
        }
        stall.nap("index reservation admitted");
    }
    assert!(
        journal
            .requests_needing_resolution()
            .iter()
            .any(|(_, request)| request.id == Some(serde_json::json!(902))),
        "the original request keeps its continuation while the index receipt is pending"
    );
    // Pause the host continuation until the reservation has replaced the file; registration
    // cannot have been submitted because the retained spawn has not consumed its index receipt.
    let path = tasty_utils::path::tasty_home()
        .unwrap()
        .join("child-terminals.json");
    let parent = session.core_state.local_workspaces()[0].all_surface_ids()[0].to_string();
    loop {
        let reserved = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value["next_index"][&parent].as_u64())
            .is_some_and(|next| next > 0);
        if reserved {
            break;
        }
        stall.nap("index reservation replacement");
    }
    let (entered, ready) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timed_out = expired.clone();
    let holder = std::thread::spawn(move || {
        let _participant = tasty_utils::file_replace::lock(&path).unwrap();
        entered.send(()).unwrap();
        if matches!(
            wait.recv_timeout(Duration::from_secs(5)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            timed_out.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let child = published_before_submit(&mut journal, &mut session);
    assert!(
        !expired.load(std::sync::atomic::Ordering::SeqCst),
        "publication must not synchronously wait on the file participant"
    );
    assert_eq!(outputs(&session, child), 0);
    assert!(matches!(
        rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(!journal.pauses_observation());
    release.send(()).unwrap();
    holder.join().unwrap();
    let response = reply(&mut journal, &mut session, &rx);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(response.id, serde_json::json!(902));
    assert!(
        rx.try_recv().is_err(),
        "the original caller receives one response"
    );
}
