//! Only short, supported managed-tool inputs cross the TUI bridge.
use super::store::Record;
use anyhow::{Result, bail};
use serde_json::{Value, json};

const MAX_TOOL_INPUT: usize = 1000;

pub fn cli(record: &Record, args: &[&str]) -> Vec<String> {
    [
        record.cli_path.as_str(),
        "--port-file",
        record.port_file.as_str(),
    ]
    .into_iter()
    .chain(args.iter().copied())
    .map(str::to_owned)
    .collect()
}

pub fn create(record: &Record) -> Result<Value> {
    let argv = cli(
        record,
        &["codex", "managed", "get", "--id", &record.relay_id],
    );
    let command = json!({"env":{"TASTY_HOME":record.cli_home},"argv":argv});
    let prompt = format!(
        "Relay parent {}, id {}. Require empty TASTY_SESSION_TOKEN/TASTY_AGENT_ID; never unset them. Run safely quoted argv with scoped env: {}. Follow returned instructions. On terminal task OR stopping error notify parent via codex_tui.send_message_to_thread. No blind retry. Final only after callback outcome or explicit error.",
        record.parent_thread,
        record.relay_id,
        serde_json::to_string(&command)?
    );
    bounded(&prompt)?;
    Ok(json!({"prompt":prompt,"title":"Tasty result relay"}))
}

pub fn callback(record: &Record) -> Result<Value> {
    let prompt = serde_json::to_string(
        &json!({"kind":"tasty_task_result_reference","relay_id":record.relay_id,
        "workspace_id":record.workspace_id,"task_id":record.task_id,"relay_thread":record.relay_thread,
        "notice":"Inspect this task and relay. This reference does not assert completion, success or delivery."}),
    )?;
    bounded(&prompt)?;
    Ok(
        json!({"tool":"codex_tui.send_message_to_thread","arguments":{"threadId":record.parent_thread,"prompt":prompt}}),
    )
}

pub fn descriptor(record: &Record) -> Result<Value> {
    let workspace = record.workspace_id.to_string();
    Ok(
        json!({"relay_id":record.relay_id,"parent_thread":record.parent_thread,
        "relay_thread":record.relay_thread,"workspace_id":record.workspace_id,"task_id":record.task_id,
        "phase":if record.relay_thread.is_some(){"attached"}else{"prepared"},
        "expected_auth_mode":"local","instance_id":record.instance_id,
        "cli_env":{"TASTY_HOME":record.cli_home},
        "instructions":concat!(
            "This is metadata, not proof of task completion or delivery. Preserve all credentials. ",
            "If TASTY_SESSION_TOKEN or TASTY_AGENT_ID is nonempty, stop with unsupported_auth_mode; never unset it. ",
            "Check CODEX_THREAD_ID against relay_thread. If attachment is pending, re-run get_argv at most once per second for 30 seconds; then report attachment_timeout. A mismatch is a failure. ",
            "Use cli_env as a scoped environment for every exact argv array, safely quoted, so plugin manifest discovery and IPC address the same instance; do not substitute PATH, home or port file. ",
            "First run task_get_argv. A missing task or denied request is a failure, not success; notify the parent before stopping when the managed tool is available. ",
            "While the task is not terminal, run await_argv, then task_get_argv. Each wait is bounded; continue only while this relay remains authorized, and do not finalize merely because one wait timed out. ",
            "Read state.kind: succeeded, failed, cancelled and skipped are terminal; preserve their differences. Unknown is nonterminal and requires human intervention: notify the parent and stop waiting without claiming task completion. ",
            "After terminal OR an error that stops this relay, run callback_argv. It returns a neutral inspection reference, not a completion claim or sent message. If callback_argv itself fails, use parent_thread and relay_id to send a short factual failure via the managed tool. ",
            "Call the returned codex_tui.send_message_to_thread with the exact threadId and prompt once. ",
            "Do not replace the call with user input or PTY text. Do not retry an ambiguous send. ",
            "Only then finalize with the actual tool outcome; task success, tool acceptance and parent consumption are separate. ",
            "On cancellation, stop this relay; cancelling a waiter does not cancel the underlying task."),
        "get_argv":cli(record,&["codex","managed","get","--id",&record.relay_id]),
        "task_get_argv":cli(record,&["agent","task-get","--workspace-id",&workspace,"--id",&record.task_id,"--json"]),
        "await_argv":cli(record,&["agent","task-await","--workspace-id",&workspace,"--id",&record.task_id,"--timeout-ms","30000"]),
        "callback_argv":cli(record,&["codex","managed","callback","--id",&record.relay_id])}),
    )
}

fn bounded(value: &str) -> Result<()> {
    if value.len() > MAX_TOOL_INPUT {
        bail!("managed_tool_input_exceeds_1000_bytes");
    }
    Ok(())
}
