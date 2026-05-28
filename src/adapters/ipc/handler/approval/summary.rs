//! `approval` IPC: summary 도메인.

use super::*;

pub fn handle_summary_set(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match params.get("workspace_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'workspace_id'"),
    };
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'content' (string)"),
    };
    let scope = Scope::Workspace(workspace_id);
    let value = MemoryValue::Text(content);
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    match with_store(|s| s.put(tasty_memory::HOST_OWNER, &scope, SUMMARY_KEY, &value, &opts)) {
        Some(Ok(_)) => JsonRpcResponse::success(id, json!({ "workspace_id": workspace_id })),
        Some(Err(e)) => JsonRpcResponse::error(id, -32603, format!("summary set failed: {e}")),
        None => JsonRpcResponse::error(id, -32603, "memory store unavailable"),
    }
}

/// `approval.summary.get` — workspace 의 요약을 반환. 없으면 `content: null`.
pub fn handle_summary_get(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match params.get("workspace_id").and_then(|v| v.as_u64()) {
        Some(v) => v as u32,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'workspace_id'"),
    };
    let scope = Scope::Workspace(workspace_id);
    let entry = with_store(|s| s.get(&scope, SUMMARY_KEY));
    match entry {
        Some(Ok(Some(e))) => {
            let content = match e.value {
                MemoryValue::Text(t) => Some(t),
                MemoryValue::Json(v) => v.as_str().map(str::to_string),
                _ => None,
            };
            JsonRpcResponse::success(
                id,
                json!({
                    "workspace_id": workspace_id,
                    "content": content,
                    "updated_at": e.updated_at,
                }),
            )
        }
        Some(Ok(None)) => {
            JsonRpcResponse::success(id, json!({ "workspace_id": workspace_id, "content": null }))
        }
        Some(Err(e)) => JsonRpcResponse::error(id, -32603, format!("summary get failed: {e}")),
        None => JsonRpcResponse::error(id, -32603, "memory store unavailable"),
    }
}
