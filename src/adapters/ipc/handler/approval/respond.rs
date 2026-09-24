//! 승인 요청에 응답하고 허용된 권한을 적용한다.

use super::*;

pub fn handle_respond(
    core: &mut crate::core::Core,
    engine: &mut crate::core::CoreState,
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

    match core.respond_approval(engine, &req_id, choice.clone(), by, comment) {
        Ok(change) => {
            persist_record(core, &change.record);
            // 권한 부여에 실패해도 승인 응답은 보존한다. agent 재시도 때 다시 권한을 요청한다.
            apply_elevation_grant_if_any(core, &change.record, &choice);
            crate::adapters::ipc::handler::memory::written(core, id, record_to_json(&change.record))
        }
        Err(e) => map_error(id, e),
    }
}
