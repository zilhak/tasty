//! 요청과 caller에 묶인 게이트 통과 증거. wire에서 만들거나 역직렬화할 수 없다.
use super::{CallerContext, JsonRpcRequest, JsonRpcResponse};
use crate::core::{Core, CoreState};
use crate::state::AppState;

/// 라우팅 전에 권한·cap·rate 검사 및 허용 관측을 끝낸 요청.
pub(crate) struct CheckedRequest<'a> {
    request: &'a JsonRpcRequest,
    caller: &'a CallerContext,
}

impl<'a> CheckedRequest<'a> {
    pub(crate) fn request(&self) -> &'a JsonRpcRequest {
        self.request
    }

    pub(crate) fn caller(&self) -> &'a CallerContext {
        self.caller
    }
}

/// 모든 진입점의 공통 게이트. 거부는 한 번 기록하고, 허용만 한 번 계측한다.
pub(crate) fn check_request<'a>(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    request: &'a JsonRpcRequest,
    caller: &'a CallerContext,
) -> Result<CheckedRequest<'a>, JsonRpcResponse> {
    let canonical = crate::ipc::alias::canonicalize(&request.method);
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let ws = engine.workspaces.get(state.active_workspace).map(|w| w.id);
    if let Some(response) =
        super::check_permission_gate(core, state, engine, caller, canonical, ws, &id)
            .or_else(|| super::check_cap_gate(core, engine, caller, canonical, ws, &id))
            .or_else(|| super::check_rate_limit_gate(core, engine, caller, canonical, ws, &id))
    {
        return Err(response);
    }
    super::record_telemetry_and_audit(core, state, engine, caller, canonical, &request.params, ws);
    Ok(CheckedRequest { request, caller })
}

/// 창/parked engine이 전혀 없는 GUI 부팅·종료 구간에는 Local만 진입 가능하다.
/// Local의 기존 부팅 예외는 유지한다. Agent를 관측 문맥 없이 통과시키지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn check_without_engine<'a>(
    request: &'a JsonRpcRequest,
    caller: &'a CallerContext,
) -> Result<CheckedRequest<'a>, JsonRpcResponse> {
    if matches!(caller, CallerContext::Local) {
        return Ok(CheckedRequest { request, caller });
    }
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    Err(JsonRpcResponse::error(
        id,
        -32000,
        "no application state available for IPC admission",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tasty_plugin_manifest::Permission;

    fn agent(permissions: &[Permission]) -> CallerContext {
        CallerContext::Agent {
            agent_id: "gate-probe".into(),
            permissions: Arc::new(permissions.iter().cloned().collect()),
        }
    }

    fn request(method: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params: json!({}),
            session_token: None,
        }
    }

    fn budget(core: &Core) {
        core.rate_limit_set(
            "gate-probe".into(),
            "ipc_calls".into(),
            1,
            86_400_000,
            Some(1),
            super::super::telemetry::now_ms(),
        )
        .unwrap();
    }

    fn observations(core: &mut Core, state: &mut AppState, engine: &mut CoreState) -> usize {
        let mut req = request("telemetry.summary");
        req.params = json!({"agent": "gate-probe", "metric": "ipc_calls"});
        super::super::handle_with_caller(core, state, engine, &req, &CallerContext::Local)
            .result
            .unwrap()["total_events"]
            .as_u64()
            .unwrap() as usize
    }

    #[test]
    fn checked_and_direct_handlers_consume_and_observe_once() {
        let _home = crate::test_support::TastyHomeGuard::new();
        for prechecked in [true, false] {
            let mut core = super::super::cli_entry_tests::test_core();
            let (mut state, mut engine) = crate::state::tests::test_state();
            let caller = agent(&[Permission::SurfaceRead]);
            let req = request("surface.kinds");
            budget(&core);
            let response = if prechecked {
                let checked = check_request(&mut core, &mut state, &mut engine, &req, &caller)
                    .unwrap_or_else(|_| panic!("first request must pass"));
                super::super::handle_checked_request(&mut core, &mut state, &mut engine, &checked)
            } else {
                super::super::handle_with_caller(&mut core, &mut state, &mut engine, &req, &caller)
            };
            assert!(response.error.is_none(), "{response:?}");
            let second =
                super::super::handle_with_caller(&mut core, &mut state, &mut engine, &req, &caller);
            assert_eq!(second.error.unwrap().code, -32010);
            assert_eq!(observations(&mut core, &mut state, &mut engine), 1);
        }
    }

    #[test]
    fn permission_denial_does_not_consume_or_observe_allow() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let req = request("surface.kinds");
        budget(&core);
        let denied =
            super::super::handle_with_caller(&mut core, &mut state, &mut engine, &req, &agent(&[]));
        let error = denied.error.unwrap();
        assert_eq!(error.code, -32001);
        assert!(error.data.unwrap()["approval_id"].is_string());
        assert_eq!(observations(&mut core, &mut state, &mut engine), 0);
        let allowed = super::super::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &agent(&[Permission::SurfaceRead]),
        );
        assert!(allowed.error.is_none());
        assert_eq!(observations(&mut core, &mut state, &mut engine), 1);
    }

    // 압력 계측의 경계. 큐 대기는 게이트 **앞**에서 재고(거부된 요청도 큐에 앉아
    // 있었으므로 그 시간은 실재한다) handler 실행 시간은 게이트 **뒤**에서 잰다.
    // 둘을 같은 자리에서 재면 거부가 실행 비용으로 보이고, 느린 응답의 원인을
    // 적체와 handler 중 어느 쪽으로도 고를 수 없게 된다.
    #[test]
    fn only_a_request_that_passed_the_gate_is_timed_as_a_handler() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let req = request("surface.kinds");
        budget(&core);

        let denied =
            super::super::handle_with_caller(&mut core, &mut state, &mut engine, &req, &agent(&[]));
        assert!(denied.error.is_some(), "권한 없는 호출은 거부된다");
        assert_eq!(
            core.pressure().snapshot().handler_calls,
            0,
            "거부는 handler 를 돌리지 않았으므로 실행 시간에 세지 않는다"
        );

        let allowed = super::super::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &agent(&[Permission::SurfaceRead]),
        );
        assert!(allowed.error.is_none());
        assert_eq!(
            core.pressure().snapshot().handler_calls,
            1,
            "통과한 요청 하나가 한 번 세져야 한다"
        );
    }

    #[test]
    fn local_remains_exempt_from_consumption_and_allow_observation() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let req = request("surface.kinds");
        budget(&core);
        for _ in 0..3 {
            assert!(
                super::super::handle_with_caller(
                    &mut core,
                    &mut state,
                    &mut engine,
                    &req,
                    &CallerContext::Local
                )
                .error
                .is_none()
            );
        }
        assert_eq!(observations(&mut core, &mut state, &mut engine), 0);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn no_engine_allows_only_local_bootstrap() {
        let req = request("surface.kinds");
        assert!(check_without_engine(&req, &CallerContext::Local).is_ok());
        assert!(check_without_engine(&req, &agent(&[Permission::SurfaceRead])).is_err());
    }
}
