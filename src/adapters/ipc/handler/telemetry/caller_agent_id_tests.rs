//! Local 호출자가 봉투의 `caller_agent_id` 로 밝힌 ID 가 텔레메트리 기록에 쓰이는지 확인한다.
//! 이 값은 권한·rate limit 에 쓰이지 않는다(ADR-0073).

use serde_json::json;

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcRequest;

fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        caller_agent_id: None,
        response_timeout_ms: None,
        idempotency_key: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
        session_token: None,
    }
}

#[test]
fn a_local_record_without_agent_uses_the_claimed_id_or_host() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    for (caller, want) in [
        (CallerContext::local_claiming(Some("agent_a")), "agent_a"),
        (CallerContext::local(), "_host"),
        (CallerContext::local_claiming(Some("bad id!")), "_host"),
    ] {
        let resp = crate::adapters::ipc::handler::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &request("telemetry.record", json!({ "metric": "probe", "value": 1 })),
            &caller,
        );
        assert!(resp.error.is_none(), "{:?}", resp.error);
        assert_eq!(resp.result.expect("result")["agent"], want);
    }
}

#[test]
fn a_local_call_with_a_claimed_id_is_counted_under_that_id() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let claimed = CallerContext::local_claiming(Some("agent_a"));
    let resp = crate::adapters::ipc::handler::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request("system.info", json!({})),
        &claimed,
    );
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let summary = crate::adapters::ipc::handler::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request("telemetry.summary", json!({ "metric": "ipc_calls" })),
        &CallerContext::local(),
    );
    let entries = summary.result.expect("summary")["entries"].clone();
    let agents: Vec<&str> = entries
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|e| e["agent"].as_str())
        .collect();
    assert_eq!(agents, ["agent_a"], "{entries}");
}

/// 자기 신고 ID 는 Local 의 권한·memory owner 를 바꾸지 않는다.
#[test]
fn a_claimed_id_changes_neither_permissions_nor_the_memory_owner() {
    let claimed = CallerContext::local_claiming(Some("com_tasty_claude"));
    assert!(claimed.permissions().is_none());
    assert_eq!(claimed.owner(), tasty_memory::HOST_OWNER);
    assert!(claimed.ensure_allowed("surface.send").is_ok());
}
