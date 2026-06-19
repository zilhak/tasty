//! `remote.passkey.*` IPC — Passkey(자격증명) CRUD.
//!
//! **값 마스킹 정책(ADR-0016 / decision 7)**: AI agent/원격은 passkey 의 **값(경로/내용)을
//! 읽을 수 없다.** list/get 은 name + kind 만 반환하고 path 는 절대 싣지 않으며, 파일 내용은
//! 어떤 응답에도 포함되지 않는다. 등록(add)은 허용 — 쓰기는 비밀을 *받는* 것이지 노출이
//! 아니다. inline 값은 `~/.tasty/passkeys/<name>` 0600 파일로 materialize 된다.

use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::Passkeys;

/// 값 마스킹된 passkey 메타(name + kind 만). path/내용은 절대 싣지 않는다.
fn passkey_meta(p: &tasty_remote_profiles::Passkey) -> Value {
    json!({ "name": p.name, "kind": p.kind })
}

/// `remote.passkey.list` → name + kind 목록(값 마스킹).
pub(crate) fn handle_list(id: Value) -> JsonRpcResponse {
    let passkeys = Passkeys::load();
    let arr: Vec<_> = passkeys.passkeys.iter().map(passkey_meta).collect();
    JsonRpcResponse::success(id, json!({ "passkeys": arr }))
}

/// `remote.passkey.get` { name } → name + kind(값 마스킹).
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

/// `remote.passkey.add` { name, kind, value } → upsert.
/// - kind=path: value = 사용자 소유 파일 경로(참조만).
/// - kind=inline: value = 비밀 — `~/.tasty/passkeys/<name>` 0600 파일로 materialize.
///
/// 응답에 value 를 절대 포함하지 않는다(쓰기 전용).
pub(crate) fn handle_add(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let Some(kind) = params.get("kind").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'kind' parameter (path|inline)");
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
            return JsonRpcResponse::invalid_params(id, format!("unknown kind '{other}' (path|inline)"));
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

/// `remote.passkey.remove` { name } → 제거(inline 이면 관리 파일도 삭제).
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
