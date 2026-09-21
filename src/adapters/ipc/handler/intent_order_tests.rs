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

/// 알림 · 팝업의 제목까지 가르는 이름. 상한의 승인 알림(`… — 승인 필요`)과 요청 핸들러의
/// 알림은 둘 다 `source: "host"` 라 `label` 로는 안 갈린다.
fn detailed_labels(state: &crate::state::AppState) -> Vec<String> {
    state
        .pending_intents
        .iter()
        .map(|intent| match &intent.body {
            Intent::Domain(DomainIntent::PushNotification { source, title, .. })
                if source == "host" =>
            {
                if title.contains("승인 필요") {
                    format!("PushNotification:{source}:approval")
                } else {
                    format!("PushNotification:{source}:{title}")
                }
            }
            Intent::Ui(crate::intent::UiIntent::OpenPopup { .. }) => "OpenPopup".into(),
            _ => label(intent),
        })
        .collect()
}

/// 운영과 같은 SQLite memory 를 쓰는 `Core`. 상한은 memory 의 목록 순서(키 오름차순 —
/// 등록 순서)대로 평가되는데, 시험용 `InMemoryStorage` 는 그 순서를 `HashMap` 에 맡겨
/// 실행마다 바뀐다. 여러 상한의 발화 순서를 보는 시험은 운영의 순서를 써야 한다.
fn core_with_ordered_memory() -> crate::core::Core {
    let store = tasty_memory::MemoryStore::open_in_memory().expect("in-memory sqlite");
    super::cli_entry_tests::test_core_builder()
        .with_memory(Arc::new(std::sync::Mutex::new(store)))
        .build()
        .expect("test Core")
}

/// 같은 agent · metric 에 `notify` 와 `require_approval` 상한을 이 순서로 건다.
fn set_notify_and_approval_caps(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    agent: &str,
    metric: &str,
) {
    for action in ["notify", "require_approval"] {
        let resp = super::handle_with_caller(
            core,
            state,
            engine,
            &request(
                "telemetry.cap.set",
                json!({
                    "agent": agent,
                    "metric": metric,
                    "threshold": 1,
                    "action": action,
                }),
            ),
            &CallerContext::Local,
        );
        assert!(
            resp.error.is_none(),
            "cap({action}) 등록 실패: {:?}",
            resp.error
        );
    }
    assert!(state.pending_intents.is_empty());
}

/// 한 평가에서 `notify` 와 `require_approval` 상한이 함께 발화하면 **발화 순서대로**
/// 쌓인다 — 먼저 발화한 상한 알림이 출구에 남은 채 승인 팝업(창 큐에 즉시 적재)에
/// 앞질리면 안 된다. 게이트(IPC 호출 수) 갈래.
#[test]
fn a_notify_cap_precedes_the_approval_popup_fired_by_the_same_gate() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = core_with_ordered_memory();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = state.focused_surface_id(&engine).expect("focused surface");
    set_notify_and_approval_caps(
        &mut core,
        &mut state,
        &mut engine,
        "order-probe",
        "ipc_calls",
    );

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

    #[cfg(feature = "gui")]
    let expected = [
        "PushNotification:telemetry.cap",
        "OpenPopup",
        "PushNotification:host:approval",
        "PushNotification:host:t",
    ]
    .as_slice();
    #[cfg(not(feature = "gui"))]
    let expected = ["PushNotification:telemetry.cap", "PushNotification:host:t"].as_slice();
    assert_eq!(detailed_labels(&state), expected);
}

/// 같은 순서 계약의 `telemetry.record` 핸들러 갈래.
#[test]
fn a_notify_cap_precedes_the_approval_popup_fired_by_a_telemetry_record() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = core_with_ordered_memory();
    let (mut state, mut engine) = crate::state::tests::test_state();
    set_notify_and_approval_caps(&mut core, &mut state, &mut engine, "order-probe", "tokens");

    let resp = super::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request(
            "telemetry.record",
            json!({ "agent": "order-probe", "metric": "tokens", "value": 1 }),
        ),
        &CallerContext::Local,
    );
    assert!(resp.error.is_none(), "{:?}", resp.error);

    #[cfg(feature = "gui")]
    let expected = [
        "PushNotification:telemetry.cap",
        "OpenPopup",
        "PushNotification:host:approval",
    ]
    .as_slice();
    #[cfg(not(feature = "gui"))]
    let expected = ["PushNotification:telemetry.cap"].as_slice();
    assert_eq!(detailed_labels(&state), expected);
}
