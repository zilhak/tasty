//! A command waiting for its InProgress record has at most one worker request in flight. A second
//! ReadCommand for the same ticket would return a Finished that no owner accepts and halt the journal.
use super::*;

fn second_engine(first: &EngineSession, journal: &mut JournalApplication) -> EngineSession {
    let second = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(2),
            memory: tasty_memory::writer::MemoryAccess::inline(Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            ))),
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
    second
}

#[test]
fn concurrent_creation_completions_do_not_double_submit_read_command() {
    let (mut first, mut journal) = boot();
    let mut second = second_engine(&first, &mut journal);
    let mut stall = StallBudget::new(&journal);
    while !journal.is_ready(second.id) {
        journal
            .poll_bootstrap(&mut [&mut first, &mut second], None)
            .unwrap();
        stall.nap("second engine bootstrap");
    }
    // Different engines let both creations wait InProgress at once. Followers join after both
    // leaders so their unbound scope cannot act as a barrier.
    let one = request(
        "workspace.create",
        serde_json::json!({"name":"one"}),
        Some("double-one"),
        1,
    );
    let two = request(
        "workspace.create",
        serde_json::json!({"name":"two"}),
        Some("double-two"),
        2,
    );
    let receivers = [
        send(&mut journal, one.clone()),
        send(&mut journal, two.clone()),
        send(&mut journal, one),
        send(&mut journal, two),
    ];
    let mut responses: [Vec<JsonRpcResponse>; 4] = Default::default();
    while responses.iter().any(Vec::is_empty) {
        let polled = journal.poll_bootstrap(&mut [&mut first, &mut second], None);
        assert!(polled.is_ok(), "journal halted: {polled:?}");
        // Any creation, forward, or resource cleanup that finishes refreshes every waiting
        // command, independent of whether that command's read is still with the worker. Stand in
        // for such an unrelated completion between every pump turn.
        journal.refresh_in_progress_commands();
        for (ticket, request) in journal.requests_needing_resolution() {
            let session = if request.params["name"] == "one" {
                &first
            } else {
                &second
            };
            journal.resolve_workspace_creation(ticket, session, None);
        }
        for (receiver, received) in receivers.iter().zip(responses.iter_mut()) {
            if let Ok(response) = receiver.try_recv() {
                received.push(response);
            }
        }
        stall.nap("concurrent workspace creations");
    }
    // Drain any late completion; a second Finished for an answered ticket would halt here.
    for _ in 0..8 {
        let polled = journal.poll_bootstrap(&mut [&mut first, &mut second], None);
        assert!(polled.is_ok(), "journal halted: {polled:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!journal.is_halted());
    for (receiver, received) in receivers.iter().zip(responses.iter_mut()) {
        received.extend(receiver.try_iter());
        assert_eq!(received.len(), 1, "{received:?}");
        assert!(received[0].error.is_none(), "{received:?}");
    }
    assert_eq!(responses[0][0].result, responses[2][0].result);
    assert_eq!(responses[1][0].result, responses[3][0].result);
    assert!(responses[2][0].idempotent_replay && responses[3][0].idempotent_replay);
    assert!(journal.commands.pending.is_empty());
}

#[test]
fn a_waiting_command_with_a_read_in_flight_is_not_queued_again() {
    let (mut session, mut journal) = boot();
    let ticket = journal.next_ticket;
    let rx = send(
        &mut journal,
        request("workspace.update", serde_json::json!({}), None, 1),
    );
    let pending = journal.commands.pending.get_mut(&ticket).unwrap();
    // A command id the store never recorded: the read stays unanswered until the worker runs.
    pending.queued = None;
    pending.waiting_command = Some("cmd-unrecorded".into());
    journal.refresh_in_progress_commands();
    assert!(matches!(
        journal.commands.pending[&ticket].queued,
        Some(Work::ReadCommand(_))
    ));
    journal.submit_commands().unwrap();
    assert!(journal.commands.pending[&ticket].queued.is_none());
    journal.refresh_in_progress_commands();
    assert!(
        journal.commands.pending[&ticket].queued.is_none(),
        "a second ReadCommand was queued while the first is in flight"
    );
    // An InProgress answer ends the in-flight read, so the next refresh reads the command again.
    let record = tasty_event_store::CommandRecord {
        command_id: "cmd-unrecorded".into(),
        key: None,
        request_digest: Vec::new(),
        resolved: Vec::new(),
        status: tasty_event_store::CommandStatus::InProgress,
        response: None,
        batch_ids: Vec::new(),
    };
    assert!(
        journal
            .answer_command(ticket, &Ok(ResultValue::Command(record)), &mut [])
            .unwrap()
    );
    journal.refresh_in_progress_commands();
    assert!(matches!(
        journal.commands.pending[&ticket].queued,
        Some(Work::ReadCommand(_))
    ));
    // The simulated answer stood in for the worker's reply; do not submit a second read for it.
    journal.commands.pending.get_mut(&ticket).unwrap().queued = None;
    // The worker's own reply to the first read settles the request without halting.
    let mut stall = StallBudget::new(&journal);
    let response = loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if let Ok(response) = rx.try_recv() {
            break response;
        }
        stall.nap("in-flight read reply");
    };
    assert!(response.error.is_some(), "{response:?}");
    assert!(!journal.is_halted());
}
