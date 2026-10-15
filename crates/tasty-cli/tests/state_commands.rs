//! 상태 조회 명령의 추천 이름과 예전 이름이 같은 요청이 되는지 확인한다.
//! 서버 동작(즉시 반환)은 호스트 handler 시험과 격리 인스턴스 확인이 맡는다.

use clap::Parser;
use tasty_cli::Cli;

fn request(args: &[&str]) -> (String, serde_json::Value) {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
    let req = tasty_cli::request::command_to_request(&cli.command.expect("subcommand"));
    (req.method, req.params)
}

#[test]
fn pty_state_and_wait_send_the_same_immediate_query() {
    let state = request(&["tasty", "pty", "state", "--id", "3"]);
    assert_eq!(state, request(&["tasty", "pty", "wait", "--id", "3"]));
    assert_eq!(state.0, "pty.wait");
    assert_eq!(state.1, serde_json::json!({ "id": 3 }));
}

#[test]
fn barrier_state_and_await_stay_separate_queries() {
    for (sub, method) in [
        ("barrier-state", "agent.barrier_state"),
        ("barrier-await", "agent.barrier_await"),
    ] {
        let (m, params) = request(&["tasty", "agent", sub, "--workspace", "1", "--name", "b"]);
        assert_eq!(m, method);
        assert_eq!(params["name"], "b");
    }
}

#[test]
fn help_lists_state_with_its_older_name() {
    let mut root = tasty_cli::localized_command();
    let pty = root.find_subcommand_mut("pty").unwrap();
    let help = pty.render_help().to_string();
    let line = help
        .lines()
        .find(|l| l.trim_start().starts_with("state"))
        .unwrap_or_else(|| panic!("{help}"));
    assert!(line.contains("wait"), "{help}");
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("wait ")),
        "{help}"
    );
}
