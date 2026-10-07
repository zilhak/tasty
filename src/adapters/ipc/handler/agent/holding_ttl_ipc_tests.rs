//! 작업 점유 TTL 의 하한 거절과, 러너가 갱신하지 못해 잃은 점유의 경고가 IPC 로 오가는지 확인한다.

use serde_json::{Value, json};
use tasty_ipc::caller::CallerContext;
use tasty_memory::{HOST_OWNER, MemoryValue, PutOpts, Scope};

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

fn create(
    core: &mut crate::app::services::AppServices,
    ttl_ms: u64,
) -> tasty_ipc::protocol::JsonRpcResponse {
    call(
        core,
        "agent.task_create",
        json!({"workspace_id": 1, "name": "t",
               "command": {"kind": "run", "workspace_id": 1, "command": ["true"]},
               "metadata": {"lease": {"resource": "db", "ttl_ms": ttl_ms}}}),
    )
}

#[test]
fn a_task_lease_ttl_below_the_minimum_is_rejected_as_invalid_params() {
    let (mut core, _memory) = core();
    let resp = create(&mut core, 999);
    let err = resp.error.expect("짧은 TTL 을 받았다");
    assert_eq!(err.code, -32602, "{err:?}");
    assert!(err.message.contains("ttl_ms"), "{}", err.message);
    assert!(create(&mut core, 1000).result.is_some());
}

#[test]
fn task_get_carries_the_holdings_lost_before_renewal() {
    let (mut core, memory) = core();
    let task = create(&mut core, 1000).result.expect("create");
    let id = task["id"].as_str().unwrap().to_string();
    let get = |core: &mut crate::app::services::AppServices| {
        call(core, "agent.task_get", json!({"workspace_id": 1, "id": id}))
            .result
            .expect("get")
    };
    assert!(get(&mut core).get("holding_warnings").is_none());
    let warning = json!({"kind": "lease", "name": "db", "holder": id, "at_ms": 5,
                         "message": "lease 'db' is no longer held"});
    memory
        .lock()
        .unwrap()
        .put(
            HOST_OWNER,
            &Scope::Workspace(1),
            &format!("tasty.agent.holding_warning.{id}"),
            &MemoryValue::Json(json!([warning.clone()])),
            &PutOpts::default(),
        )
        .unwrap();
    assert_eq!(get(&mut core)["holding_warnings"], json!([warning]));
}
