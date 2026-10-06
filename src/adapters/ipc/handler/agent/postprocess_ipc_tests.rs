//! 후처리 선언과 진행이 IPC 그래프 제출·task 조회로 오가는지 확인한다.

use serde_json::{Value, json};
use tasty_ipc::caller::CallerContext;

use super::super::cli_entry_tests::test_core_builder;

type Memory = std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>;

fn core() -> (crate::app::services::AppServices, Memory) {
    let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
    let memory: Memory = std::sync::Arc::new(std::sync::Mutex::new(store));
    let core = test_core_builder()
        .with_memory(memory.clone())
        .build()
        .expect("core");
    (core, memory)
}

fn call_raw(
    core: &mut crate::app::services::AppServices,
    method: &str,
    params: Value,
) -> tasty_ipc::protocol::JsonRpcResponse {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let req = tasty_ipc::protocol::JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        session_token: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    };
    super::super::handle_with_caller(core, &mut state, &mut engine, &req, &CallerContext::Local)
}

fn call(core: &mut crate::app::services::AppServices, method: &str, params: Value) -> Value {
    let resp = call_raw(core, method, params);
    resp.result
        .unwrap_or_else(|| panic!("{method} failed: {:?}", resp.error))
}

fn graph(postprocess: Value) -> Value {
    json!({"workspace_id": 1, "graph": {"contract_version": 2, "tasks": [
        {"id": "judge", "output_schema": {"type": "enum", "values": ["pass", "revise"]},
         "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
         "postprocess": postprocess}
    ]}})
}

#[test]
fn a_graph_carries_the_postprocess_declaration_and_reports_its_phase() {
    let (mut core, memory) = core();
    let pp = json!({"command": ["judge-cli", "--json"], "timeout_ms": 60000,
                    "stdin": {"draft": {"from": "raw", "pointer": "/execution/stdout/text"}},
                    "stdout": {"pointer": "/verdict"},
                    "retry": {"max_retries": 1, "delay_ms": 1000}});
    call(&mut core, "agent.task_graph_submit", graph(pp.clone()));
    let task = call(
        &mut core,
        "agent.task_get",
        json!({"workspace_id": 1, "id": "judge"}),
    );
    assert_eq!(task["contract"]["postprocess"]["command"], pp["command"]);
    assert_eq!(
        task["contract"]["postprocess"]["stdout"]["format"],
        json!("json")
    );
    // 본 작업이 끝나기 전에는 후처리 단계가 아니다.
    assert!(task.get("phase").is_none(), "{task}");

    // runner 가 본 작업을 끝낸 상태를 저장소로 만든다(IPC 에는 Running 전이가 없다).
    {
        let mut mem = memory.lock().expect("memory");
        let seq = std::sync::atomic::AtomicU64::new(1000);
        let mut store = tasty_agent::TaskStore::new(&mut *mem, tasty_memory::HOST_OWNER, &seq);
        let id = "judge".to_string();
        store
            .set_state(1, &id, tasty_agent::TaskState::Running, 1)
            .expect("running");
        let main = tasty_agent::TaskResult {
            exit_code: Some(0),
            output: None,
            error: None,
        };
        store
            .complete(
                1,
                &id,
                tasty_agent::task::Completion::succeeded(None, main),
                2,
            )
            .expect("main completion");
    }
    let task = call(
        &mut core,
        "agent.task_get",
        json!({"workspace_id": 1, "id": "judge"}),
    );
    assert_eq!(task["state"]["kind"], json!("running"));
    assert_eq!(task["phase"], json!("postprocessing"));
    assert_eq!(
        task["attempt"]["postprocess"]["phase"]["state"],
        json!("pending")
    );
    assert_eq!(
        task["attempt"]["postprocess"]["execution"]["exit_code"],
        json!(0)
    );
}

#[test]
fn an_invalid_postprocess_is_rejected_at_its_location() {
    let (mut core, _memory) = core();
    let e = call_raw(
        &mut core,
        "agent.task_graph_submit",
        graph(json!({"command": ["judge-cli"], "timeout_ms": 0})),
    )
    .error
    .expect("rejected");
    assert_eq!(e.code, -32602);
    assert_eq!(
        e.data.expect("data")["location"],
        json!("/tasks/0/postprocess/timeout_ms")
    );
}
