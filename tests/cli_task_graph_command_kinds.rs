//! CLI `agent task-graph-submit` 가 만든 요청이 command 종류마다 그래프 해석을 통과하는지 확인한다.
//! CLI 는 `--workspace-id` 를 그 필드가 있는 run·agent command 에만 채운다. 그래프 해석은 다른
//! 종류의 command 에 있는 `workspace_id` 를 모르는 키로 거절하므로, 둘이 어긋나면 이 시험이 잡는다.

use clap::Parser;
use tasty_agent::task::TaskGraphSpec;

const COMMANDS: [&str; 5] = [
    r#"{"kind":"run","command":["true"]}"#,
    r#"{"kind":"agent","provider":"claude","instruction":"i","session":{"kind":"existing","surface_id":1}}"#,
    r#"{"kind":"wait_barrier","name":"b"}"#,
    r#"{"kind":"reduce","inputs":[],"strategy":{"kind":"all"}}"#,
    r#"{"kind":"custom","ipc_method":"system.ping"}"#,
];

const INLINE_FALLBACK: &str =
    r#"{"kind":"fallback","inline":{"name":"f","command":{"kind":"run","command":["true"]}}}"#;

fn cli_graph(graph: &str) -> serde_json::Value {
    let cli = tasty_cli::Cli::try_parse_from([
        "tasty",
        "agent",
        "task-graph-submit",
        "--workspace-id",
        "3",
        "--graph",
        graph,
    ])
    .expect("cli args");
    let request = tasty_cli::request::command_to_request(&cli.command.expect("command"));
    request.params["graph"].clone()
}

#[test]
fn a_cli_graph_of_every_command_kind_is_accepted() {
    for command in COMMANDS {
        for on_failure in [None, Some(INLINE_FALLBACK)] {
            let mut task = format!(r#"{{"id":"x","command":{command}"#);
            if let Some(f) = on_failure {
                task.push_str(&format!(r#","on_failure":{f}"#));
            }
            let graph = format!(r#"{{"contract_version":2,"tasks":[{task}}}]}}"#);
            let parsed = TaskGraphSpec::from_json(&cli_graph(&graph));
            assert!(parsed.is_ok(), "{graph}: {parsed:?}");
        }
    }
}
