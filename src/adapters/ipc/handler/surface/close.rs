use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// 공용 close 본문 — [`handle_surface_close`] 와 [`handle_surface_close_self`] 가 공유한다.
/// 실행은 `core::structural_exec::close_surface` 이고, 여기는 응답 JSON 을 만든다.
///
/// IPC 요청 진입점은 에이전트 경로라 `save_snapshot=false` 다(되돌리기 스택은 사용자 행동의
/// 것이다). 원격 holder 가 forward 한 close 는 이 함수를 거치지 않고 forward 실행이
/// 도메인 함수를 `save_snapshot=true` 로 직접 부른다 — 그 축의 근거는 도메인 함수의 문서.
fn close_surface_via_intent(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    surface_id: u32,
) -> JsonRpcResponse {
    match crate::core::structural_exec::close_surface(core, state, engine, surface_id, false) {
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

/// 원격 attach 가 **하드 점유** 중이면 거절 응답을 돌린다.
///
/// 점유 중에는 그 터미널을 원격 사용자가 쓰고 있고, close 는 비가역이다 — 되돌리기 스택에
/// 남는 것은 살아 있는 PTY 가 아니라 같은 명령으로 새 세션을 여는 레시피다. `workspace.close`
/// 가 같은 판정을 워크스페이스 단위로 하고(ADR-0120 ④), GUI 경로는
/// `AppState::refuse_if_hard_occupied` 가 소유한다. 에이전트에게는 토스트가 아니라 사유가
/// 실린 에러를 준다.
///
/// # 왜 params 플래그가 아니라 **호출 경로**로 면제하는가
///
/// 정당한 예외가 하나 있다: holder 자신이 원격에서 보낸 close 는 통과해야 한다
/// (`attach_runtime::execute_forwarded_structural_op`). 그 예외를 `params` 의 플래그로
/// 표현하면 **아무 에이전트나 같은 키를 실어 우회한다** — params 는 호출자가 만든다.
/// 그래서 면제는 데이터가 아니라 **어느 함수를 부르느냐**로 표현한다: 요청 진입점
/// ([`handle_surface_close`] · [`handle_surface_close_self`])은 이 검사를 지나고, holder
/// 경로는 IPC 진입점을 아예 거치지 않고 도메인 실행(`core::structural_exec::close_surface`)을
/// 직접 부른다. 그 실행은 점유를 묻지 않는다 — 점유는 요청 진입의 게이트다.
///
/// `surface.close_self` 가 예외가 아닌 것도 같은 이유다 — 그 메서드는 호출자를 확인하지
/// 않고 params 의 `surface_id` 를 그대로 받으므로, 예외로 두면 그대로 우회 통로가 된다.
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
    state: &mut AppState,
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
    close_surface_via_intent(core, state, engine, id, surface_id)
}

/// Close the calling surface itself. Only way for a surface to close itself.
pub(crate) fn handle_surface_close_self(
    core: &mut crate::core::Core,
    state: &mut AppState,
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
    close_surface_via_intent(core, state, engine, id, surface_id)
}

#[cfg(test)]
mod hard_occupancy_tests {
    //! `surface.close` 는 원격이 하드 점유한 surface 를 거절한다.
    //!
    //! `workspace.close` 는 이미 거절했는데 이쪽은 열려 있었다 — 워크스페이스 단위로는
    //! 막히고 surface 단위로는 그대로 죽는 비대칭이었다(실측). GUI 쪽 같은 규칙은
    //! `AppState::refuse_if_hard_occupied` 가 소유하고, 그 짝 회귀는
    //! `state::tests::close_refuses_hard_occupied` 에 있다.
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

    /// `close_self` 는 예외가 아니다 — 호출자를 확인하지 않고 params 의 id 를 받으므로
    /// 예외로 두면 그대로 우회 통로가 된다.
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

    /// 통과 대조 — 점유가 없으면 같은 요청이 여전히 닫는다.
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
