//! A close pauses observation except while every operation it prepared only awaits receipts.
use super::close_pauses;
use tasty_core::OperationId;

fn op(n: u32) -> OperationId {
    OperationId(format!("cmd-1-{n}/prepare/0"))
}

#[test]
fn an_uncommitted_close_pauses() {
    assert!(close_pauses(true, false, false, &[op(1)], |_| true));
}

#[test]
fn a_committed_close_whose_operations_await_receipts_does_not_pause() {
    assert!(!close_pauses(true, false, true, &[op(1), op(2)], |_| true));
}

#[test]
fn a_close_pauses_once_any_operation_leaves_the_receipt_wait() {
    // Finish publishes the final facts; after it the reply is still pending.
    assert!(close_pauses(true, false, true, &[op(1), op(2)], |o| *o == op(1)));
}

#[test]
fn a_committed_close_without_known_operations_keeps_pausing() {
    assert!(close_pauses(true, false, true, &[], |_| true));
}

#[test]
fn a_replacement_always_pauses_and_other_commands_never_do() {
    assert!(close_pauses(false, true, true, &[op(1)], |_| true));
    assert!(!close_pauses(false, false, true, &[op(1)], |_| true));
}
