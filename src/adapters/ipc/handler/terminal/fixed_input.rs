//! 활성화를 기다릴 수 있는 terminal 입력의 wire 해석과 응답 변환.
//! 대상 고정과 실행은 `app::terminal_input`이 typed 값으로 맡는다.
use super::*;
use crate::app::services::impl_attach::SurfaceSendOutcome;
use crate::app::terminal_input::{
    ChildTarget, PreparedTerminalInput, RespawnOptions, TerminalInputResult,
};

/// tell·broadcast·cwd 없는 respawn의 대상과 본문·옵션을 요청 시점에 값으로 확정한다.
/// 그 밖의 요청은 None이며 일반 입력 봉투로 다시 실행된다.
pub(crate) fn prepare_input(
    engine: &EngineRef<'_>,
    request: &crate::ipc::protocol::JsonRpcRequest,
) -> Result<Option<PreparedTerminalInput>, JsonRpcResponse> {
    let id = request.id.clone().unwrap_or_default();
    if request.method == "terminal.tell" {
        let (surface, text) = decode_tell(&request.params, &id)?;
        return Ok(Some(PreparedTerminalInput::Tell {
            surface,
            text: text.to_owned(),
        }));
    }
    if !matches!(
        request.method.as_str(),
        "terminal.broadcast" | "terminal.respawn"
    ) {
        return Ok(None);
    }
    let parent = resolve_parent(engine, &request.params, &id)?;
    if request.method == "terminal.broadcast" {
        let text = require_str(&request.params, "text", &id)?;
        let role = optional_str(&request.params, "role");
        let targets = engine
            .runtime
            .child_terminals
            .list_children(&parent)
            .iter()
            .filter(|entry| {
                role.as_ref()
                    .is_none_or(|role| entry.role.as_ref() == Some(role))
            })
            .map(|entry| ChildTarget::fix(engine, parent.clone(), entry))
            .collect();
        return Ok(Some(PreparedTerminalInput::Broadcast { targets, text }));
    }
    if request
        .params
        .get("cwd")
        .is_some_and(|value| value.is_string())
    {
        return Ok(None);
    }
    let index = require_u32(&request.params, "child", &id)?;
    let entry = engine
        .runtime
        .child_terminals
        .find_child(&parent, index)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(
                id,
                child_not_found_message(&engine.runtime.child_terminals, &parent, index),
            )
        })?;
    Ok(Some(PreparedTerminalInput::Respawn {
        target: ChildTarget::fix(engine, parent, entry),
        options: respawn_options(&request.params),
    }))
}

/// respawn 옵션을 값으로 읽는다. 문자열이 아닌 값은 지정하지 않은 것으로 본다.
pub(crate) fn respawn_options(params: &Value) -> RespawnOptions {
    RespawnOptions {
        command: optional_str(params, "command"),
        role: optional_str(params, "role"),
        nickname: optional_str(params, "nickname"),
    }
}

/// 입력 결과를 기존 응답 형태와 오류 코드·문구로 바꾼다.
pub(crate) fn input_response(
    engine: &EngineRef<'_>,
    id: Value,
    result: TerminalInputResult,
) -> JsonRpcResponse {
    match result {
        TerminalInputResult::Told { surface } => {
            JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface }))
        }
        TerminalInputResult::Rejected { surface, reason } => {
            submit_rejected_response(id, surface, reason)
        }
        TerminalInputResult::Broadcast { children } => JsonRpcResponse::success(
            id,
            json!({ "sent_count": children.len(), "children": children }),
        ),
        TerminalInputResult::Respawned { surface, index } => JsonRpcResponse::success(
            id,
            json!({ "child_surface_id": surface, "child_index": index }),
        ),
        TerminalInputResult::ChildMissing { parent, index } => JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.runtime.child_terminals, &parent, index),
        ),
        TerminalInputResult::InterruptFailed {
            surface,
            outcome: outcome @ SurfaceSendOutcome::HardOccupied,
        } => JsonRpcResponse::invalid_params(
            id,
            super::super::surface::send_fail_message(surface, outcome),
        ),
        TerminalInputResult::InterruptFailed { .. } => {
            JsonRpcResponse::internal_error(id, "No terminal found".to_string())
        }
        TerminalInputResult::CommandFailed { surface, outcome } => JsonRpcResponse::invalid_params(
            id,
            super::super::surface::send_fail_message(surface, outcome),
        ),
        TerminalInputResult::TargetChanged => JsonRpcResponse::invalid_params(
            id,
            "child terminal target changed while awaiting activation",
        ),
    }
}
