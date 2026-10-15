//! 승인 요청에 응답한다. 허용된 권한은 App 승인 계약이 적용한다.

use super::*;

pub fn handle_respond(
    core: &mut crate::app::services::AppServices,
    _engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let req_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => ApprovalId(s.to_string()),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'id'"),
    };
    let choice = match params.get("choice").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing 'choice'"),
    };
    let comment = params
        .get("comment")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let by = responder_from_caller(caller);

    match core.respond_approval(&req_id, choice, by, comment) {
        Ok(transition) => {
            let body = record_to_json(&transition.change.record);
            reply_after_record(core, id, transition, body)
        }
        Err(e) => map_error(id, e),
    }
}
