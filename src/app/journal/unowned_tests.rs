use super::*;
use crate::app::journal::stall_budget::StallBudget;

fn boot() -> (EngineSession, JournalApplication) {
    let mut settings = crate::settings::Settings::default();
    crate::app::journal::test_shell::use_quiet_shell(&mut settings);
    let mut session = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory: tasty_memory::writer::MemoryAccess::inline(Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            ))),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        settings,
    )
    .unwrap();
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal
        .begin_engine(
            &session,
            EngineSelection::Slot {
                slot: 1,
                resume: false,
            },
        )
        .unwrap();
    let mut stall = StallBudget::new(&journal);
    while !journal.is_ready(session.id) {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        stall.nap("bootstrap");
    }
    (session, journal)
}

/// 어느 소유자도 등록하지 않은 ticket 의 완료를 넣고 halt 사유를 돌려받는다.
fn halt_with_unowned(work: Work) -> (JournalApplication, String) {
    let (mut session, mut journal) = boot();
    journal
        .worker
        .submit(Request { ticket: 999, work })
        .unwrap();
    let mut stall = StallBudget::new(&journal);
    let error = loop {
        if let Err(error) = journal.poll_bootstrap(&mut [&mut session], None) {
            break error;
        }
        stall.nap("unowned completion halt");
    };
    (journal, error)
}

#[test]
fn unowned_completion_reports_ticket_and_result_kind() {
    let (journal, error) = halt_with_unowned(Work::ReadEngine("structure:slot-1".into()));
    assert!(error.contains("999"), "{error}");
    assert!(error.contains("result Engine"), "{error}");
    assert!(!error.contains("bootstrap completion"), "{error}");
    assert_eq!(journal.halted.as_deref(), Some(error.as_str()));
}

#[test]
fn unowned_failure_reports_the_error_text() {
    let (_, error) = halt_with_unowned(Work::ReadCommand("never-admitted".into()));
    assert!(error.contains("999"), "{error}");
    assert!(error.contains("original command missing"), "{error}");
    assert!(!error.contains("bootstrap completion"), "{error}");
}

#[test]
fn halted_requests_carry_the_halt_cause() {
    let (journal, error) = halt_with_unowned(Work::ReadEngine("structure:slot-1".into()));
    let request: crate::ipc::protocol::JsonRpcRequest = serde_json::from_value(
        serde_json::json!({"jsonrpc":"2.0","method":"tree","params":{},"id":1}),
    )
    .unwrap();
    let message = journal
        .reject_halted_request(&request)
        .unwrap()
        .error
        .unwrap()
        .message;
    assert!(message.contains("restart"), "{message}");
    assert!(message.contains(&error), "{message}");
}

/// 정산된 command 의 ticket 으로 늦은 완료가 오면 halt 사유가 그 command 의 마지막 제출을 가리킨다.
#[test]
fn a_late_completion_names_the_settled_commands_last_submission() {
    let (mut session, mut journal) = boot();
    let ticket = journal.next_ticket;
    let request: crate::ipc::protocol::JsonRpcRequest = serde_json::from_value(serde_json::json!({
        "jsonrpc":"2.0",
        "method":"workspace_category.create",
        "params":{"name":"late"},
        "id":1,
    }))
    .unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    assert!(journal.admit_ipc(
        &crate::ipc::server::IpcCommand::new(request, tx),
        &crate::ipc::caller::CallerContext::local()
    ));
    let mut stall = StallBudget::new(&journal);
    let response = loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        for (ticket, _) in journal.requests_needing_resolution() {
            journal.resolve_ipc_for_engine(ticket, &session);
        }
        if let Ok(response) = rx.try_recv() {
            break response;
        }
        stall.nap("category create reply");
    };
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(journal.commands.pending_count(), 0);

    journal
        .worker
        .submit(Request {
            ticket,
            work: Work::ReadEngine("structure:slot-1".into()),
        })
        .unwrap();
    let error = loop {
        if let Err(error) = journal.poll_bootstrap(&mut [&mut session], None) {
            break error;
        }
        stall.nap("late completion halt");
    };
    assert!(
        error.contains(&format!("ticket {ticket} has no owner")),
        "{error}"
    );
    assert!(
        error.contains("last command submission Resolve "),
        "{error}"
    );
    assert!(
        error.contains("openings=0 commands=0 creations=0"),
        "{error}"
    );
}

/// 보내기 전 forward의 원격 큐 확보가 worker에서 실패하면 journal이 멈추고, 그 forward를 기다리던
/// run은 멈춘 원인으로 끝난다.
#[cfg(feature = "gui")]
#[test]
fn a_failed_forward_claim_halts_and_fails_the_waiting_run() {
    let (mut session, mut journal) = boot();
    let run = crate::request_runs::begin("markdown.navigate", None, serde_json::Value::Null);
    journal.forwards.insert(
        journal.next_ticket,
        super::forward::Forward::claiming(
            session.id,
            "structure:slot-1".into(),
            tasty_core::OperationId("never-admitted".into()),
            crate::request_runs::RunGuard::new(run, super::forward::FORWARD_ENDED),
        ),
    );
    journal.next_ticket += 1;
    let mut stall = StallBudget::new(&journal);
    let error = loop {
        if let Err(error) = journal.poll_bootstrap(&mut [&mut session], None) {
            break error;
        }
        stall.nap("forward claim halt");
    };
    let ended = crate::request_runs::lookup(run, None).unwrap();
    assert_eq!(ended["state"], "failed");
    assert_eq!(
        ended["result"],
        format!("the remote change was not sent because the journal stopped: {error}")
    );
}

/// 보낼 차례가 된 forward가 있는 poll에서 journal이 멈추면 그 forward는 보내지 않는다. run은
/// 미송신 실패로 끝났으므로 원격에 적용되면 결과와 실제가 어긋난다.
#[cfg(feature = "gui")]
#[test]
fn a_halt_keeps_a_ready_forward_from_being_sent() {
    let (mut session, mut journal) = boot();
    let (sender, queue) = tasty_remote::connection::channel();
    let run = crate::request_runs::begin("markdown.navigate", None, serde_json::Value::Null);
    let draft = super::forward::Draft {
        engine: session.id,
        stream: "structure:slot-1".into(),
        response: crate::ipc::protocol::JsonRpcResponse::success(
            serde_json::Value::Null,
            serde_json::json!({"forwarded": true}),
        ),
        target: crate::app::attach_client::RemoteTarget {
            engine: session.id,
            workspace: 1,
            surface: None,
            sender,
            port: 1,
            remote_workspace: 1,
        },
        local_anchor: 1,
        remote_anchor: 1,
        op_id: 0,
        payload: b"{}".to_vec(),
        focus: None,
        silent_failure: true,
        closes_workspace: false,
        completion: Some(crate::request_runs::RunGuard::new(
            run,
            super::forward::FORWARD_ENDED,
        )),
    };
    let lease = crate::runtime::journal_product::EffectLease {
        effect_id: "effect".into(),
        operation: tasty_core::OperationId("operation".into()),
        stream: "structure:slot-1".into(),
        runtime_epoch: 0,
        resource_generation: 0,
        attempt: 1,
    };
    journal.forwards.insert(
        journal.next_ticket,
        super::forward::Forward::ready(draft, lease),
    );
    journal.next_ticket += 1;
    journal
        .worker
        .submit(Request {
            ticket: 999,
            work: Work::ReadEngine("structure:slot-1".into()),
        })
        .unwrap();
    let mut stall = StallBudget::new(&journal);
    while journal.poll_bootstrap(&mut [&mut session], None).is_ok() {
        stall.nap("ready forward halt");
    }
    assert!(journal.take_ready_forwards().is_empty());
    assert!(queue.try_recv().is_err(), "nothing reached the connection");
    let ended = crate::request_runs::lookup(run, None).unwrap();
    assert_eq!(ended["state"], "failed");
}
