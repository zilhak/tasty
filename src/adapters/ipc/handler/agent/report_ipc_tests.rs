//! report append·조회가 IPC 로 오가고, report 가 binding 입력이 되지 못하는지 확인한다.

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

fn call_as(
    core: &mut crate::app::services::AppServices,
    caller: &CallerContext,
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
    super::super::handle_with_caller(core, &mut state, &mut engine, &req, caller)
}

fn call(core: &mut crate::app::services::AppServices, method: &str, params: Value) -> Value {
    let resp = call_raw(core, method, params);
    resp.result
        .unwrap_or_else(|| panic!("{method} failed: {:?}", resp.error))
}

fn with_store<R>(memory: &Memory, f: impl FnOnce(&mut tasty_agent::TaskStore) -> R) -> R {
    let mut mem = memory.lock().expect("memory");
    let seq = std::sync::atomic::AtomicU64::new(5000);
    let mut store = tasty_agent::TaskStore::new(&mut *mem, tasty_memory::HOST_OWNER, &seq);
    f(&mut store)
}

#[test]
fn a_running_attempt_takes_notes_by_its_address_and_the_dag_report_shows_them() {
    let (mut core, memory) = core();
    call(
        &mut core,
        "agent.task_graph_submit",
        json!({"workspace_id": 1, "graph": {"contract_version": 2, "tasks": [
            {"id": "a", "metadata": {"dag": "rep"}, "output_schema": {"type": "int64"},
             "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}}]}}),
    );
    // runner 의 dispatch 와 Running 전이를 저장소로 만든다.
    let id = "a".to_string();
    with_store(&memory, |s| {
        s.issue_report_token(
            1,
            &id,
            tasty_agent::task::report::ReportToken {
                attempt: 1,
                token: "t0k".into(),
            },
        )
        .expect("token");
        s.set_state(1, &id, tasty_agent::TaskState::Running, 1)
            .expect("running");
    });

    let ok = call(
        &mut core,
        "agent.report_append",
        json!({"address": "1/1/run/t0k/a", "text": "checked 3 files"}),
    );
    assert_eq!(ok["result"], json!("stored"), "{ok}");

    // 다른 토큰은 같은 회차라도 받지 않는다.
    let e = call_raw(
        &mut core,
        "agent.report_append",
        json!({"address": "1/1/run/other/a", "text": "x"}),
    )
    .error
    .expect("rejected");
    assert_eq!(e.code, -32018);
    assert_eq!(e.data.expect("data")["reason"], json!("token_mismatch"));
    let e = call_raw(
        &mut core,
        "agent.report_append",
        json!({"address": "not an address", "text": "x"}),
    )
    .error
    .expect("rejected");
    assert_eq!(e.code, -32602);

    // 시험 상태에는 살아 있는 workspace 가 없어 workspace 를 직접 준다.
    let report = call(
        &mut core,
        "agent.dag_report",
        json!({"id": "d:rep", "workspace_id": 1}),
    );
    assert_eq!(report["dag"], json!("d:rep"), "{report}");
    let task = &report["tasks"][0];
    assert_eq!(task["task_id"], json!("a"));
    assert_eq!(task["auto"]["kind"], json!("run"));
    let entry = &task["attempts"][0]["custom"]["entries"][0];
    assert_eq!(entry["text"], json!("checked 3 files"));
    assert_eq!(entry["source"], json!("run"));

    // 회차 하나만 고르려면 task 가 필요하다.
    let e = call_raw(
        &mut core,
        "agent.dag_report",
        json!({"id": "d:rep", "workspace_id": 1, "attempt": 1}),
    )
    .error
    .expect("rejected");
    assert_eq!(e.code, -32602);
    let e = call_raw(
        &mut core,
        "agent.dag_report",
        json!({"id": "d:rep", "workspace_id": 1, "task": "nope"}),
    )
    .error
    .expect("rejected");
    assert_eq!(e.code, -32602);

    // 회차가 끝나면 주소가 닫힌다.
    with_store(&memory, |s| {
        s.complete(
            1,
            &id,
            tasty_agent::task::Completion::failed(None, "boom".into()),
            2,
        )
        .expect("failed");
    });
    let e = call_raw(
        &mut core,
        "agent.report_append",
        json!({"address": "1/1/run/t0k/a", "text": "late"}),
    )
    .error
    .expect("rejected");
    let data = e.data.expect("data");
    assert_eq!(data["reason"], json!("closed"));
    assert_eq!(data["state"], json!("failed"));
}

/// report 는 출력 문서에 속하지 않는다. binding 은 출력의 위치만 가리키며 report 를 고를 칸이 없다.
#[test]
fn a_binding_cannot_read_a_report() {
    let (mut core, _memory) = core();
    let graph = |source: Value| {
        json!({"workspace_id": 1, "graph": {"contract_version": 2, "tasks": [
            {"id": "p", "output_schema": {"type": "int64"},
             "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}},
            {"id": "c", "input_schema": {"type": "object", "fields": {"n": {"type": "string"}}},
             "bindings": {"n": source},
             "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}}]}})
    };
    for source in [
        json!({"from_task": "p", "pointer": "", "report": true}),
        json!({"from_task": "p", "pointer": "/report"}),
    ] {
        let resp = call_raw(&mut core, "agent.task_graph_submit", graph(source.clone()));
        let e = resp.error.unwrap_or_else(|| panic!("accepted: {source}"));
        assert_eq!(e.code, -32602, "{source}: {}", e.message);
    }
}

fn session(agent_id: &str) -> CallerContext {
    CallerContext::Agent {
        agent_id: agent_id.into(),
        permissions: std::sync::Arc::new(
            [tasty_plugin_manifest::Permission::AgentManage]
                .into_iter()
                .collect(),
        ),
    }
}

/// 세션 토큰의 agent 는 자기 세션에 지시를 보낸 회차에만 쓴다. 토큰이 맞아도 다른 세션은 거절한다.
#[test]
fn an_agent_session_writes_only_to_the_attempt_bound_to_its_session() {
    let (mut core, memory) = core();
    call(
        &mut core,
        "agent.task_graph_submit",
        json!({"workspace_id": 1, "graph": {"contract_version": 2, "tasks": [
            {"id": "s", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}}]}}),
    );
    let id = "s".to_string();
    with_store(&memory, |s| {
        s.issue_report_token(
            1,
            &id,
            tasty_agent::task::report::ReportToken {
                attempt: 1,
                token: "tk".into(),
            },
        )
        .expect("token");
        s.set_state(1, &id, tasty_agent::TaskState::Running, 1)
            .expect("running");
    });
    let append = |core: &mut _, caller: &CallerContext| {
        call_as(
            core,
            caller,
            "agent.report_append",
            json!({"address": "1/1/agent/tk/s", "text": "note"}),
        )
    };
    // 지시를 보낸 세션이 없으면 세션 호출은 받지 않는다.
    let e = append(&mut core, &session("claude_s7"))
        .error
        .expect("rejected");
    assert_eq!(e.code, -32018);
    assert_eq!(e.data.expect("data")["reason"], json!("not_the_session"));

    core.tasks
        .agent_turns()
        .bind(
            7,
            tasty_task_runtime::agent_turns::TurnBinding::new(
                1,
                id.clone(),
                tasty_agent::task::attempt::attempt_id(&id, 1),
                "claude".into(),
                "turn".into(),
                true,
            ),
        )
        .expect("bind");
    let ok = append(&mut core, &session("claude_s7"));
    assert!(ok.error.is_none(), "{:?}", ok.error);
    assert_eq!(ok.result.expect("result")["result"], json!("stored"));
    // 다른 세션은 주소의 토큰을 알아도 쓰지 못한다.
    let e = append(&mut core, &session("claude_s8"))
        .error
        .expect("rejected");
    assert_eq!(e.data.expect("data")["reason"], json!("not_the_session"));
    // 세션이 아닌 agent id 는 회차에 묶을 수 없다.
    let e = append(&mut core, &session("child-1"))
        .error
        .expect("rejected");
    assert_eq!(e.code, -32001);
    // 사용자 CLI 는 토큰만 본다.
    let local = append(&mut core, &CallerContext::local());
    assert!(local.error.is_none(), "{:?}", local.error);

    // 끝난 회차에는 묶인 세션이라도 closed 로 거절된다.
    with_store(&memory, |s| {
        s.complete(
            1,
            &id,
            tasty_agent::task::Completion::failed(None, "boom".into()),
            2,
        )
        .expect("failed");
    });
    let e = append(&mut core, &session("claude_s7"))
        .error
        .expect("rejected");
    assert_eq!(e.data.expect("data")["reason"], json!("closed"));
}
