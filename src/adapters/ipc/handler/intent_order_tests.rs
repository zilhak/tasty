//! IPC 요청이 낸 intent 가 **어떤 순서로** 창 큐에 쌓이는가.
//!
//! 핸들러는 intent 를 창 큐에 직접 넣지 않고 요청 하나의 출구(`IntentOutbox`)에 넣는다.
//! 진입점이 요청이 끝날 때 출구를 창 큐 끝으로 옮긴다(`window_port` 모듈 문서). 이 시험들이
//! 고정하는 것은 그 이동이 외부에서 보이는 순서를 바꾸지 않는다는 것이다 — 이미 쌓여 있던
//! 것 뒤에, 게이트가 낸 것 다음에 핸들러가 낸 것, 요청 순서대로.

use serde_json::json;
use std::sync::Arc;
use tasty_plugin_manifest::Permission;

use crate::core::intent::DomainIntent;
use crate::intent::{DispatchedIntent, Intent};
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcRequest;

fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
        session_token: None,
    }
}

/// 큐의 한 칸을 비교 가능한 이름으로. 도메인 intent 는 variant 와 알림의 `source`,
/// 나머지는 variant 이름만.
fn label(intent: &DispatchedIntent) -> String {
    match &intent.body {
        Intent::Domain(DomainIntent::PushNotification { source, .. }) => {
            format!("PushNotification:{source}")
        }
        Intent::Domain(DomainIntent::SetTerminalMark { .. }) => "SetTerminalMark".into(),
        Intent::RestoreClosedItem => "RestoreClosedItem".into(),
        other => format!("{:?}", std::mem::discriminant(other)),
    }
}

fn labels(state: &crate::state::AppState) -> Vec<String> {
    state.pending_intents.iter().map(label).collect()
}

/// 핸들러가 낸 intent 는 이미 쌓여 있던 것 **뒤에**, 요청 순서대로 붙는다.
#[test]
fn handler_intents_append_after_the_queue_in_request_order() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = super::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = state.focused_surface_id(&engine).expect("focused surface");
    state.dispatch_intent(Intent::RestoreClosedItem.from_user_shortcut("sentinel"));

    for req in [
        request(
            "notification.create",
            json!({ "title": "t", "body": "b", "surface_id": sid }),
        ),
        request("surface.set_mark", json!({ "surface_id": sid })),
    ] {
        let resp = super::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        assert!(
            resp.error.is_none(),
            "{} 실패: {:?}",
            req.method,
            resp.error
        );
    }

    assert_eq!(
        labels(&state),
        [
            "RestoreClosedItem",
            "PushNotification:host",
            "SetTerminalMark"
        ],
        "요청이 낸 intent 는 기존 큐 뒤에 요청 순서대로 쌓여야 한다"
    );
}

/// 한 요청 안에서는 **게이트가 낸 intent(상한 알림)가 핸들러가 낸 것보다 먼저** 쌓인다 —
/// 게이트가 핸들러보다 먼저 돌기 때문이다. 둘이 서로 다른 출구에 모인 뒤에도 그 순서가
/// 유지되는지 고정한다.
#[test]
fn gate_intents_precede_the_handler_intents_of_the_same_request() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = super::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = state.focused_surface_id(&engine).expect("focused surface");

    // `order-probe` 의 IPC 호출 수가 1 에 닿으면 알림을 내는 상한.
    let set = super::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request(
            "telemetry.cap.set",
            json!({
                "agent": "order-probe",
                "metric": "ipc_calls",
                "threshold": 1,
                "action": "notify",
            }),
        ),
        &CallerContext::Local,
    );
    assert!(set.error.is_none(), "cap 등록 실패: {:?}", set.error);
    assert!(state.pending_intents.is_empty());

    let agent = CallerContext::Agent {
        agent_id: "order-probe".into(),
        permissions: Arc::new([Permission::Notification].into_iter().collect()),
    };
    let resp = super::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request(
            "notification.create",
            json!({ "title": "t", "body": "b", "surface_id": sid }),
        ),
        &agent,
    );
    assert!(resp.error.is_none(), "{:?}", resp.error);

    assert_eq!(
        labels(&state),
        ["PushNotification:telemetry.cap", "PushNotification:host"],
        "게이트의 상한 알림이 핸들러의 알림보다 먼저 쌓여야 한다"
    );
}

/// 거절된 요청은 intent 를 남기지 않는다 — 출구가 비어 있으면 창 큐도 그대로다.
#[test]
fn a_rejected_request_leaves_the_queue_untouched() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = super::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    state.dispatch_intent(Intent::RestoreClosedItem.from_user_shortcut("sentinel"));

    let resp = super::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request("surface.set_mark", json!({})),
        &CallerContext::Local,
    );
    assert!(resp.error.is_some(), "surface_id 없는 set_mark 는 거절된다");
    assert_eq!(labels(&state), ["RestoreClosedItem"]);
}
