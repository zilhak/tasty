//! `memory.*` / `memory.secret.*` IPC 핸들러. `~/.tasty/memory.db`
//! (tasty-memory 크레이트) 위에서 동작하며, 모든 호출은 메인 스레드에서 순차
//! 처리된다.
//!
//! 스코프는 JSON 파라미터 `scope` 로 받는다 (예: `"global"`, `"surface:3"`).
//! 값은 `value` (텍스트/JSON) 또는 `value_b64` (base64 binary).
//!
//! `owner` 는 [`CallerContext`] 에서 도출하며 plugin 이 인자로 명시할 수 없다.

use serde_json::{Value, json};
use tasty_memory::{
    ListOpts, MemoryArea, MemoryEntry, MemoryError, MemoryStats, MemoryValue, PutOpts, Scope,
    with_store,
};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

fn require_scope(params: &Value, id: &Value) -> Result<Scope, JsonRpcResponse> {
    let raw = params
        .get("scope")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing 'scope' parameter"))?;
    Scope::parse(raw)
        .map_err(|s| JsonRpcResponse::invalid_params(id.clone(), format!("invalid scope: {s}")))
}

fn require_key<'a>(params: &'a Value, id: &Value) -> Result<&'a str, JsonRpcResponse> {
    params
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing 'key' parameter"))
}

fn optional_scope(params: &Value, id: &Value) -> Result<Option<Scope>, JsonRpcResponse> {
    match params.get("scope").and_then(|v| v.as_str()) {
        None => Ok(None),
        Some(raw) => Scope::parse(raw).map(Some).map_err(|s| {
            JsonRpcResponse::invalid_params(id.clone(), format!("invalid scope: {s}"))
        }),
    }
}

fn parse_value(params: &Value, id: &Value) -> Result<MemoryValue, JsonRpcResponse> {
    let content_type = params
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| match params.get("value") {
            Some(Value::String(_)) => "text/plain",
            Some(_) => "application/json",
            None if params.get("value_b64").is_some() => "application/octet-stream",
            None => "application/json",
        });
    match content_type {
        "text/plain" => match params.get("value") {
            Some(Value::String(s)) => Ok(MemoryValue::Text(s.clone())),
            Some(_) => Err(JsonRpcResponse::invalid_params(
                id.clone(),
                "content_type=text/plain requires 'value' to be a string",
            )),
            None => Err(JsonRpcResponse::invalid_params(
                id.clone(),
                "Missing 'value' parameter",
            )),
        },
        "application/json" => match params.get("value") {
            Some(v) => Ok(MemoryValue::Json(v.clone())),
            None => Err(JsonRpcResponse::invalid_params(
                id.clone(),
                "Missing 'value' parameter",
            )),
        },
        "application/octet-stream" => match params.get("value_b64").and_then(|v| v.as_str()) {
            Some(b64) => decode_b64(b64)
                .map(MemoryValue::Binary)
                .map_err(|e| JsonRpcResponse::invalid_params(id.clone(), e)),
            None => Err(JsonRpcResponse::invalid_params(
                id.clone(),
                "content_type=application/octet-stream requires 'value_b64'",
            )),
        },
        other => Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("unsupported content_type: {other}"),
        )),
    }
}

fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("invalid base64: length must be multiple of 4".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buf = [0u8; 4];
    for chunk in bytes.chunks(4) {
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => 64,
                _ => return Err(format!("invalid base64 char: {:?}", b as char)),
            };
        }
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        out.push((buf[0] << 2) | (buf[1] >> 4));
        if pad < 2 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if pad < 1 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
    Ok(out)
}

fn encode_b64(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(b & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b = (bytes[i] as u32) << 16;
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

fn value_to_json(v: &MemoryValue) -> Value {
    match v {
        MemoryValue::Text(s) => json!({
            "kind": "text",
            "content_type": "text/plain",
            "value": s,
        }),
        MemoryValue::Json(j) => json!({
            "kind": "json",
            "content_type": "application/json",
            "value": j,
        }),
        MemoryValue::Binary(b) => json!({
            "kind": "binary",
            "content_type": "application/octet-stream",
            "value_b64": encode_b64(b),
            "size": b.len(),
        }),
    }
}

/// Regular entry — `owner` 가 응답에 포함된다.
fn entry_to_json(entry: &MemoryEntry) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("scope".into(), json!(entry.scope));
    obj.insert("key".into(), json!(entry.key));
    obj.insert("version".into(), json!(entry.version));
    obj.insert("created_at".into(), json!(entry.created_at));
    obj.insert("updated_at".into(), json!(entry.updated_at));
    obj.insert("expires_at".into(), json!(entry.expires_at));
    if let Some(owner) = &entry.owner {
        obj.insert("owner".into(), json!(owner));
    }
    if let Some(map) = value_to_json(&entry.value).as_object() {
        for (k, vv) in map {
            obj.insert(k.clone(), vv.clone());
        }
    }
    Value::Object(obj)
}

/// Secret entry — plugin 에게 `owner` 차원을 노출하지 않으므로 무조건 생략.
fn secret_entry_to_json(entry: &MemoryEntry) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("scope".into(), json!(entry.scope));
    obj.insert("key".into(), json!(entry.key));
    obj.insert("version".into(), json!(entry.version));
    obj.insert("created_at".into(), json!(entry.created_at));
    obj.insert("updated_at".into(), json!(entry.updated_at));
    obj.insert("expires_at".into(), json!(entry.expires_at));
    if let Some(map) = value_to_json(&entry.value).as_object() {
        for (k, vv) in map {
            obj.insert(k.clone(), vv.clone());
        }
    }
    Value::Object(obj)
}

fn stats_to_json(s: &MemoryStats) -> Value {
    json!({
        "scope": s.scope,
        "entries": s.entries,
        "bytes": s.bytes,
    })
}

fn map_error(id: Value, err: MemoryError) -> JsonRpcResponse {
    use MemoryError::*;
    match err {
        NotFound { scope, key } => {
            JsonRpcResponse::error(id, -32004, format!("not_found: {scope}/{key}"))
        }
        CasConflict { expected, actual } => JsonRpcResponse::error(
            id,
            -32005,
            format!("cas_conflict: expected v{expected}, got v{actual}"),
        ),
        OwnedByOther { owner } => {
            JsonRpcResponse::error(id, -32006, format!("owned_by_other: {owner}"))
        }
        QuotaExceeded { area, used, limit } => {
            let area_str = match area {
                MemoryArea::Regular => "regular",
                MemoryArea::Secret => "secret",
            };
            JsonRpcResponse::error(
                id,
                -32007,
                format!("quota_exceeded ({area_str}): used {used}, limit {limit}"),
            )
        }
        InvalidKey(msg) => JsonRpcResponse::invalid_params(id, format!("invalid_key: {msg}")),
        InvalidScope(msg) => JsonRpcResponse::invalid_params(id, format!("invalid_scope: {msg}")),
        InvalidOwner(msg) => JsonRpcResponse::invalid_params(id, format!("invalid_owner: {msg}")),
        InvalidContentType(msg) => {
            JsonRpcResponse::invalid_params(id, format!("invalid_content_type: {msg}"))
        }
        ValueTooLarge { actual, max } => JsonRpcResponse::error(
            id,
            -32007,
            format!("value_too_large: {actual} bytes > {max}"),
        ),
        Db(e) => JsonRpcResponse::internal_error(id, format!("memory db error: {e}")),
    }
}

fn require_store(id: &Value) -> Result<(), JsonRpcResponse> {
    if tasty_memory::with_store(|_| ()).is_some() {
        Ok(())
    } else {
        Err(JsonRpcResponse::internal_error(
            id.clone(),
            "memory store not initialized",
        ))
    }
}

// ============================================================
// Regular `memory.*`
// ============================================================

pub fn handle_put(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let opts = PutOpts {
        expires_at: params.get("expires_at").and_then(|v| v.as_i64()),
        cas: params.get("cas").and_then(|v| v.as_u64()),
    };
    let owner = caller.owner().to_string();

    let result = with_store(|s| s.put(&owner, &scope, &key, &value, &opts))
        .expect("memory store presence verified");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_get(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| s.get(&scope, &key)).expect("memory store present");
    match result {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_delete(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.delete(&owner, &scope, &key, cas)).expect("memory store present");
    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_list(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = ListOpts {
        prefix: params
            .get("prefix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        limit: params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };
    let result = with_store(|s| s.list(&scope, &opts)).expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

pub fn handle_exists(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let result = with_store(|s| s.exists(&scope, &key)).expect("memory store present");
    match result {
        Ok(b) => JsonRpcResponse::success(id, json!({ "exists": b })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_count(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let prefix = params.get("prefix").and_then(|v| v.as_str());
    let result = with_store(|s| s.count(&scope, prefix)).expect("memory store present");
    match result {
        Ok(n) => JsonRpcResponse::success(id, json!({ "count": n })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_scopes(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let result = with_store(|s| s.scopes()).expect("memory store present");
    match result {
        Ok(list) => JsonRpcResponse::success(id, json!({ "scopes": list })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_stats(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match optional_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let result = with_store(|s| s.stats(scope.as_ref())).expect("memory store present");
    match result {
        Ok(stats) => JsonRpcResponse::success(id, stats_to_json(&stats)),
        Err(e) => map_error(id, e),
    }
}

/// `memory.gc` — local_only. 만료 entry 일괄 DELETE (regular + secret).
/// 응답: `{ regular: N, secret: M }`. read 경로는 항상 만료 필터를 거치므로
/// 사용자에게 보이는 동작은 변하지 않고, 디스크 정리 + quota 회복만 일어난다.
pub fn handle_gc(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let result = with_store(|s| s.purge_expired()).expect("memory store present");
    match result {
        Ok(stats) => JsonRpcResponse::success(
            id,
            json!({ "regular": stats.regular, "secret": stats.secret }),
        ),
        Err(e) => map_error(id, e),
    }
}

// ============================================================
// Secret `memory.secret.*` — owner 자동 분기, 응답에 owner 미포함
// ============================================================

pub fn handle_secret_put(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let opts = PutOpts {
        expires_at: params.get("expires_at").and_then(|v| v.as_i64()),
        cas: params.get("cas").and_then(|v| v.as_u64()),
    };
    let owner = caller.owner().to_string();

    let result = with_store(|s| s.put_secret(&owner, &scope, &key, &value, &opts))
        .expect("memory store present");
    match result {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_get(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.get_secret(&owner, &scope, &key)).expect("memory store present");
    match result {
        Ok(Some(entry)) => JsonRpcResponse::success(id, secret_entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_delete(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let cas = params.get("cas").and_then(|v| v.as_u64());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.delete_secret(&owner, &scope, &key, cas)).expect("memory store present");
    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_list(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = ListOpts {
        prefix: params
            .get("prefix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        limit: params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.list_secret(&owner, &scope, &opts)).expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(secret_entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_exists(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let key = match require_key(params, &id) {
        Ok(k) => k.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.exists_secret(&owner, &scope, &key)).expect("memory store present");
    match result {
        Ok(b) => JsonRpcResponse::success(id, json!({ "exists": b })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_count(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match require_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let prefix = params.get("prefix").and_then(|v| v.as_str());
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.count_secret(&owner, &scope, prefix)).expect("memory store present");
    match result {
        Ok(n) => JsonRpcResponse::success(id, json!({ "count": n })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_scopes(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    _params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let owner = caller.owner().to_string();
    let result = with_store(|s| s.scopes_secret(&owner)).expect("memory store present");
    match result {
        Ok(list) => JsonRpcResponse::success(id, json!({ "scopes": list })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_secret_stats(
    _state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let scope = match optional_scope(params, &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.stats_secret(&owner, scope.as_ref())).expect("memory store present");
    match result {
        Ok(stats) => JsonRpcResponse::success(id, stats_to_json(&stats)),
        Err(e) => map_error(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for input in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let encoded = encode_b64(input);
            let decoded = decode_b64(&encoded).unwrap();
            assert_eq!(decoded, input, "roundtrip failed for {input:?}");
        }
    }

    #[test]
    fn base64_invalid_inputs_rejected() {
        assert!(decode_b64("abc").is_err());
        assert!(decode_b64("ab*=").is_err());
    }
}
