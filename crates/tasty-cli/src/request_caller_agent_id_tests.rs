//! CLI 가 자기 환경의 `TASTY_AGENT_ID` 를 봉투의 `caller_agent_id` 에 싣는다(ADR-0076).
//! 환경변수를 바꾸지 않으려고 시험 바이너리를 그 값을 둔 자식 프로세스로 다시 실행한다.

use clap::Parser;

fn request_in_child(test: &str, agent_id: &str) {
    let status = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args([
            "--exact",
            &format!("request::caller_agent_id_tests::{test}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env("TASTY_TEST_CALLER_AGENT_ID_CHILD", "1")
        .env("TASTY_AGENT_ID", agent_id)
        .env_remove("TASTY_SESSION_TOKEN")
        .status()
        .expect("child");
    assert!(status.success(), "child run failed: {status}");
}

fn in_child() -> bool {
    std::env::var_os("TASTY_TEST_CALLER_AGENT_ID_CHILD").is_some()
}

fn list_info() -> tasty_ipc::protocol::JsonRpcRequest {
    let cli = crate::Cli::try_parse_from(["tasty", "list", "info"]).expect("parse");
    super::command_to_request(&cli.command.expect("command"))
}

#[test]
fn the_shell_agent_id_rides_on_the_request() {
    if !in_child() {
        return request_in_child("the_shell_agent_id_rides_on_the_request", "agent_a");
    }
    assert_eq!(list_info().caller_agent_id.as_deref(), Some("agent_a"));
}

#[test]
fn an_empty_agent_id_is_not_sent() {
    if !in_child() {
        return request_in_child("an_empty_agent_id_is_not_sent", "");
    }
    let request = list_info();
    assert_eq!(request.caller_agent_id, None);
    let line = serde_json::to_string(&request).expect("serialize");
    assert!(!line.contains("caller_agent_id"), "{line}");
}
