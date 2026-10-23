//! App response ownership after the worker's group publication halt.
use super::*;

#[test]
fn fail_all_and_late_group_finished_leave_one_response_per_original_ticket() {
    let (mut session, mut journal) = boot();
    let workspace = session.core_state.local_workspaces()[0].id;
    let mut tickets = Vec::new();
    let mut receivers = Vec::new();
    for index in 0..4 {
        tickets.push(journal.next_ticket);
        receivers.push(send(
            &mut journal,
            request(
                "workspace.update",
                serde_json::json!({"id":workspace,"name":"group"}),
                Some(if index == 3 {
                    "b3"
                } else {
                    ["b1", "b2", "b3"][index]
                }),
                index as u64 + 1,
            ),
        ));
    }
    for ticket in &tickets {
        let pending = journal.commands.pending.get_mut(ticket).unwrap();
        pending.admitted = true;
        pending.in_flight = true;
        pending.queued = None;
        pending.request.params = serde_json::Value::Null;
    }
    let first = ResultValue::Executed(crate::runtime::command_executor::Executed {
        command_id: "committed-b1".into(),
        status: tasty_event_store::CommandStatus::Completed,
        response: Some(
            serde_json::to_vec(&JsonRpcResponse::success(
                serde_json::Value::Null,
                serde_json::json!({"published":"b1"}),
            ))
            .unwrap(),
        ),
        source: crate::runtime::command_executor::Source::Committed { batch: Some(1) },
    });
    assert!(
        journal
            .answer_command(tickets[0], &Ok(first), &mut [&mut session])
            .unwrap()
    );
    assert!(receivers[0].try_recv().unwrap().error.is_none());
    journal.fail_pending_commands("B2 publication failed after group commit");
    for receiver in &receivers[1..] {
        assert!(receiver.try_recv().unwrap().error.is_some());
    }
    journal.fail_pending_commands("same terminal halt");
    for ticket in tickets {
        assert!(
            !journal
                .answer_command(
                    ticket,
                    &Err(JournalError::Halted("late group Finished".into())),
                    &mut [&mut session]
                )
                .unwrap()
        );
    }
    assert!(journal.commands.pending.is_empty());
    assert!(
        receivers
            .iter()
            .all(|receiver| receiver.try_recv().is_err())
    );
}
