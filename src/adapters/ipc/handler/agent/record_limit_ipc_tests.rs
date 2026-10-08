//! 결과 전 레코드 상한을 넘는 task 정의가 IPC 에서 `-32602` 로 거절되는지 확인한다.

use serde_json::{Value, json};
use tasty_agent::task::record_limit::MAX_RECORD_BEFORE_RESULT_BYTES;
use tasty_ipc::caller::CallerContext;

use super::super::cli_entry_tests::test_core_builder;

fn core() -> crate::app::services::AppServices {
    let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
    test_core_builder()
        .with_memory(std::sync::Arc::new(std::sync::Mutex::new(store)))
        .build()
        .expect("core")
}

fn call(
    core: &mut crate::app::services::AppServices,
    method: &str,
    params: Value,
) -> tasty_ipc::protocol::JsonRpcResponse {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let req = tasty_ipc::protocol::JsonRpcRequest {
        caller_agent_id: None,
        response_timeout_ms: None,
        idempotency_key: None,
        session_token: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    };
    super::super::handle_with_caller(core, &mut state, &mut engine, &req, &CallerContext::local())
}

fn pad() -> String {
    "m".repeat(MAX_RECORD_BEFORE_RESULT_BYTES)
}

#[test]
fn a_task_definition_over_the_record_limit_is_invalid_params() {
    let mut core = core();
    let resp = call(
        &mut core,
        "agent.task_create",
        json!({"workspace_id": 1, "name": "t",
               "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
               "metadata": {"pad": pad()}}),
    );
    let err = resp.error.expect("큰 정의를 받았다");
    assert_eq!(err.code, -32602, "{}", err.message);
    assert!(
        err.message
            .starts_with("invalid argument: task record too large: task ")
            && err.message.contains(&format!(
                "over the {MAX_RECORD_BEFORE_RESULT_BYTES} byte limit for a task before its result"
            )),
        "{}",
        err.message
    );
    let list = call(&mut core, "agent.task_list", json!({"workspace_id": 1}));
    assert_eq!(list.result.expect("list")["tasks"], json!([]));
}

#[test]
fn a_graph_task_over_the_record_limit_is_invalid_params_with_its_location() {
    let mut core = core();
    let graph = json!({"workspace_id": 1, "graph": {"contract_version": 2, "tasks": [
        {"id": "a", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}},
        {"id": "b", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
         "metadata": {"pad": pad()}}
    ]}});
    for method in ["agent.task_graph_validate", "agent.task_graph_submit"] {
        let err = call(&mut core, method, graph.clone())
            .error
            .expect("큰 정의를 받았다");
        assert_eq!(err.code, -32602, "{method}: {}", err.message);
        let data = err.data.expect("data");
        assert_eq!(data["location"], "/tasks/1", "{method}: {data}");
        assert_eq!(data["stage"], "input", "{method}: {data}");
        assert!(
            err.message
                .contains("task record too large: task b: the definition makes the task record"),
            "{method}: {}",
            err.message
        );
    }
}
