use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 도메인 close 함수를 호출하고 JSON 응답을 만든다. IPC는 복원 기록을 남기지 않는다.
/// 원격 holder 경로는 여기 대신 도메인을 직접 호출해 요청 출처별로 복원 여부를 정한다(ADR-0023).
fn close_surface_via_intent(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    surface_id: u32,
) -> JsonRpcResponse {
    match crate::core::structural_exec::close_surface(core, window, engine, surface_id, false) {
        Ok(crate::core::structural_exec::Closed {
            id: surface_id,
            closed: true,
        }) => JsonRpcResponse::success(id, json!({ "closed": true, "surface_id": surface_id })),
        Ok(crate::core::structural_exec::Closed { id: surface_id, .. }) => {
            JsonRpcResponse::success(
                id,
                json!({ "closed": false, "surface_id": surface_id, "reason": "surface not found" }),
            )
        }
        Err(f) => super::super::structural_failure_response(id, f),
    }
}

/// 원격 사용자가 점유 중인 터미널을 로컬 IPC가 닫지 못하게 한다.
/// holder의 원격 닫기는 별도 진입점에서 도메인 함수를 직접 호출한다.
/// 요청 params에 면제 플래그를 두면 누구나 우회할 수 있으므로 허용하지 않는다.
/// close_self도 호출자 검증 없이 요청 ID를 받으므로 같은 점유 검사를 거친다.
fn refuse_if_hard_occupied(
    engine: &crate::core::CoreState,
    id: &serde_json::Value,
    surface_id: u32,
) -> Option<JsonRpcResponse> {
    if !engine.attach.is_hard_occupied(surface_id) {
        return None;
    }
    Some(JsonRpcResponse::invalid_params(
        id.clone(),
        format!(
            "Surface {surface_id} is occupied by a remote attach session (hard-occupied) \
             — someone is working in that terminal right now. Release it from the \
             attaching instance first."
        ),
    ))
}

pub(crate) fn handle_surface_close(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // Prevent closing the caller's own surface — use 'close self' instead.
    if let Some(caller) = super::caller_surface_id(params)
        && caller == surface_id
    {
        return JsonRpcResponse::invalid_params(
            id,
            "Cannot close your own surface with 'close surface'. Use 'tasty close self' instead.",
        );
    }
    if let Some(refusal) = refuse_if_hard_occupied(engine, &id, surface_id) {
        return refusal;
    }
    close_surface_via_intent(core, window, engine, id, surface_id)
}

/// 일반 close의 자기 대상 방지를 거치지 않고 닫는다. 점유 검사는 동일하게 적용한다.
pub(crate) fn handle_surface_close_self(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    if let Some(refusal) = refuse_if_hard_occupied(engine, &id, surface_id) {
        return refusal;
    }
    close_surface_via_intent(core, window, engine, id, surface_id)
}

#[cfg(test)]
mod hard_occupancy_tests {
    //! 원격 점유 중인 surface는 일반 close와 close_self 모두 거절해야 한다.
    use super::*;
    use serde_json::json;

    const HOLDER: u32 = 1;

    #[test]
    fn closing_a_surface_a_remote_session_occupies_is_refused() {
        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        // 두 번째 워크스페이스 — 마지막 워크스페이스 cascade 와 얽히지 않게 한다.
        crate::core::apply_create_workspace_inner(
            &mut engine,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .expect("워크스페이스 생성");
        let target = engine.workspaces[1].all_surface_ids()[0];
        engine.attach.acquire(target, HOLDER).expect("하드 점유");

        let res = handle_surface_close(
            &mut core,
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": target }),
        );

        let err = res.error.expect("하드 점유 surface 는 거절해야 한다");
        assert!(
            err.message.contains("hard-occupied"),
            "거절 사유가 점유임을 알려야 한다: {}",
            err.message
        );
        assert!(
            engine
                .workspaces
                .iter()
                .any(|w| w.all_surface_ids().contains(&target)),
            "거절이면 surface 가 살아 있어야 한다"
        );
        assert!(
            engine.attach.is_hard_occupied(target),
            "거절 경로가 점유 상태를 건드리면 안 된다"
        );
    }

    #[test]
    fn close_self_is_not_a_bypass_for_an_occupied_surface() {
        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        crate::core::apply_create_workspace_inner(
            &mut engine,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .expect("워크스페이스 생성");
        let target = engine.workspaces[1].all_surface_ids()[0];
        engine.attach.acquire(target, HOLDER).expect("하드 점유");

        let res = handle_surface_close_self(
            &mut core,
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": target }),
        );

        assert!(
            res.error.is_some(),
            "close_self 로도 점유 surface 를 닫을 수 없어야 한다"
        );
    }

    #[test]
    fn closing_an_unoccupied_surface_still_works() {
        let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        crate::core::apply_create_workspace_inner(
            &mut engine,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .expect("워크스페이스 생성");
        let target = engine.workspaces[1].all_surface_ids()[0];

        let res = handle_surface_close(
            &mut core,
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": target }),
        );

        assert!(
            res.error.is_none(),
            "점유가 없으면 닫혀야 한다: {:?}",
            res.error
        );
    }
}
