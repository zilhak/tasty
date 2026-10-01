//! IPC 구조 변경은 호출자 종류와 무관하게 에이전트 요청으로 실행된다.
//! 사용자 포커스·닫은 항목·lifecycle의 사용자 닫기 표시를 바꾸지 않는지 확인한다.

use crate::runtime::engine_access::EngineMut;
use serde_json::json;
use std::sync::Arc;
use tasty_plugin_manifest::Permission;

use crate::core::origin::{AgentSource, IntentOrigin};
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcRequest;
use crate::state::RequestContext;

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

fn callers() -> [CallerContext; 3] {
    let permissions: Arc<std::collections::HashSet<Permission>> =
        Arc::new([Permission::SurfaceWrite].into_iter().collect());
    [
        CallerContext::Local,
        CallerContext::Agent {
            agent_id: "child:1".into(),
            permissions: permissions.clone(),
        },
        CallerContext::Plugin {
            plugin_id: "origin-probe".into(),
            permissions,
        },
    ]
}

/// 활성 workspace의 선택 pane과 각 pane 선택 탭의 선택 surface.
fn user_focus(state: &RequestContext, engine: &crate::core::CoreState) -> (u32, Option<u32>) {
    let ws = state.active_workspace(engine);
    (
        state.navigation.pane_id(ws).unwrap(),
        state.focused_surface_id(engine),
    )
}

fn call(
    core: &mut crate::app::services::AppServices,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    caller: &CallerContext,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let resp = super::handle_with_caller(core, state, engine, &request(method, params), caller);
    if let Some(e) = resp.error {
        panic!("{method} · {caller:?}: {}", e.message);
    }
    resp.result.expect("result")
}

#[test]
fn caller_contexts_map_to_agent_origins() {
    let [local, agent, plugin] = callers();
    for caller in [local, agent] {
        assert!(
            matches!(
                super::intent_origin_of(&caller),
                IntentOrigin::Agent {
                    source: AgentSource::Ipc
                }
            ),
            "{caller:?}"
        );
    }
    assert!(matches!(
        super::intent_origin_of(&plugin),
        IntentOrigin::Agent { source: AgentSource::Plugin(id) } if id == "origin-probe"
    ));
}

#[test]
fn an_ipc_split_keeps_the_users_focus_for_every_caller() {
    for caller in callers() {
        for level in ["pane", "surface"] {
            let _home = crate::test_support::TastyHomeGuard::new();
            let mut core = super::cli_entry_tests::test_core();
            let (mut state, mut engine_session) = crate::state::tests::test_state();
            let mut engine = engine_session.borrow_mut();
            let sid = state.focused_surface_id(&engine).expect("focused surface");
            let before = user_focus(&state, &engine);

            call(
                &mut core,
                &mut state,
                &mut engine,
                &caller,
                "split",
                json!({ "level": level, "target_surface": sid, "type": "empty" }),
            );

            assert_eq!(
                engine
                    .workspace_at(state.active_workspace_index(&engine))
                    .expect("workspace index is valid")
                    .all_surface_ids()
                    .len(),
                2,
                "{level} · {caller:?}: split이 실행되지 않았다"
            );
            assert_eq!(user_focus(&state, &engine), before, "{level} · {caller:?}");
        }
    }
}

#[test]
fn ipc_tab_and_pane_closes_leave_no_user_close_trace_for_every_caller() {
    for caller in callers() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::cli_entry_tests::test_core();
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let sid = state.focused_surface_id(&engine).expect("focused surface");
        let pane_id = state.focused_pane_id(&engine);

        let tab = call(
            &mut core,
            &mut state,
            &mut engine,
            &caller,
            "tab.create",
            json!({ "pane_id": pane_id, "type": "empty" }),
        );
        let new_sid = tab["surface_id"].as_u64().expect("surface_id") as u32;
        let tab_id = engine.find_tab_for_surface(new_sid).expect("new tab");
        let split = call(
            &mut core,
            &mut state,
            &mut engine,
            &caller,
            "split",
            json!({ "level": "pane", "target_surface": sid, "type": "empty" }),
        );
        let new_pane = split["new_pane_id"].as_u64().expect("new_pane_id") as u32;
        state.pending_lifecycle_events.clear();

        let closed = call(
            &mut core,
            &mut state,
            &mut engine,
            &caller,
            "tab.close",
            json!({ "tab_id": tab_id }),
        );
        assert_eq!(closed["closed"], true, "{caller:?}");
        let closed = call(
            &mut core,
            &mut state,
            &mut engine,
            &caller,
            "pane.close",
            json!({ "pane_id": new_pane }),
        );
        assert_eq!(closed["closed"], true, "{caller:?}");

        assert_eq!(engine.closed_items.len(), 0, "{caller:?}");
        // lifecycle 통지는 gui 빌드에만 있다.
        if cfg!(feature = "gui") {
            assert!(
                !state.pending_lifecycle_events.is_empty(),
                "{caller:?}: 닫기 lifecycle을 하나도 재지 않았다"
            );
            assert!(
                state
                    .pending_lifecycle_events
                    .iter()
                    .all(|e| !e.is_user_close),
                "{caller:?}"
            );
        }
    }
}

/// mirror workspace의 구조 변경은 원격으로 전달된다. 실패가 사용자 toast로 가지 않게 표시한다.
#[cfg(feature = "gui")] // headless에는 전달 큐를 보내는 루프가 없어 거절한다
#[test]
fn a_plugin_split_on_a_mirror_forwards_as_a_silent_agent_request() {
    let [_, _, plugin] = callers();
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = super::cli_entry_tests::test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let sid = state.focused_surface_id(&engine).expect("focused surface");
    let active = state.active_workspace_index(&engine);
    engine.make_mirror_fixture(active);

    super::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &request(
            "split",
            json!({ "level": "pane", "target_surface": sid, "type": "empty" }),
        ),
        &plugin,
    );

    assert_eq!(engine.remote.pending_structural_forward.len(), 1);
    let forwarded = &engine.remote.pending_structural_forward[0];
    assert!(forwarded.silent_failure);
    assert!(!forwarded.user_triggered);
}
