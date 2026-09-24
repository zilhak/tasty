//! 원격 자격증명 관리. 응답에는 name과 kind만 포함하고 경로·내용은 공개하지 않는다.
//! inline 값은 데이터 루트의 passkeys/<name>에 저장하며 Unix에서는 0600을 적용한다(ADR-0011).

use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::Passkeys;

fn passkey_meta(p: &tasty_remote_profiles::Passkey) -> Value {
    json!({ "name": p.name, "kind": p.kind })
}

pub(crate) fn handle_list(id: Value) -> JsonRpcResponse {
    let passkeys = Passkeys::load();
    let arr: Vec<_> = passkeys.passkeys.iter().map(passkey_meta).collect();
    JsonRpcResponse::success(id, json!({ "passkeys": arr }))
}

pub(crate) fn handle_get(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let passkeys = Passkeys::load();
    match passkeys.get(name) {
        Some(p) => JsonRpcResponse::success(id, json!({ "passkey": passkey_meta(p) })),
        None => JsonRpcResponse::error(id, -32040, format!("passkey '{name}' not found")),
    }
}

/// path는 사용자 파일을 참조하고 inline은 관리 파일에 저장한다. 값은 응답하지 않는다.
pub(crate) fn handle_add(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let Some(kind) = params.get("kind").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(
            id,
            "Missing required 'kind' parameter (path|inline)",
        );
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'value' parameter");
    };

    let mut passkeys = Passkeys::load();
    let replaced = passkeys.get(name).is_some();
    let res = match kind {
        "path" => passkeys.upsert_path(name, value.to_string()),
        "inline" => passkeys.upsert_inline(name, value),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown kind '{other}' (path|inline)"),
            );
        }
    };
    if let Err(e) = res {
        return JsonRpcResponse::invalid_params(id, format!("invalid passkey: {e}"));
    }
    match passkeys.save() {
        Ok(()) => JsonRpcResponse::success(
            id,
            json!({ "saved": true, "name": name, "kind": kind, "replaced": replaced }),
        ),
        Err(e) => JsonRpcResponse::internal_error(id, format!("failed to save passkey: {e}")),
    }
}

/// inline 자격증명이면 관리 파일도 삭제한다.
pub(crate) fn handle_remove(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let mut passkeys = Passkeys::load();
    if !passkeys.remove(name) {
        return JsonRpcResponse::error(id, -32040, format!("passkey '{name}' not found"));
    }
    match passkeys.save() {
        Ok(()) => JsonRpcResponse::success(id, json!({ "removed": true, "name": name })),
        Err(e) => JsonRpcResponse::internal_error(id, format!("failed to save passkey: {e}")),
    }
}
