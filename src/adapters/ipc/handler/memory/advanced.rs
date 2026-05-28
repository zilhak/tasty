//! 메모리 도메인 IPC 핸들러의 advanced 그룹: gc / query / export / import.

use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryEntry, MemoryValue, with_store};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::{decode_b64, entry_to_json, map_error, optional_scope, require_scope, require_store};

/// `memory.gc` — local_only. 만료 entry 일괄 DELETE (regular + secret).
/// 응답: `{ regular: N, secret: M }`. read 경로는 항상 만료 필터를 거치므로
/// 사용자에게 보이는 동작은 변하지 않고, 디스크 정리 + quota 회복만 일어난다.
pub fn handle_gc(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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

/// `memory.query` — `application/json` value 들을 dot-path 매칭으로 필터링.
/// 파라미터: `scope`, `path` (예: `"task.status"`), `equals` (임의 JSON value),
/// 그리고 list 와 동일한 `prefix`/`since`/`until`/`limit`/`offset`.
pub fn handle_query(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => {
            return JsonRpcResponse::invalid_params(id, "Missing or empty 'path' parameter");
        }
    };
    let equals = match params.get("equals") {
        Some(v) => v.clone(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'equals' parameter"),
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
        since: params.get("since").and_then(|v| v.as_i64()),
        until: params.get("until").and_then(|v| v.as_i64()),
        offset: params
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };
    let result =
        with_store(|s| s.query(&scope, &path, &equals, &opts)).expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

/// `memory.export` — regular 영역 entry 를 dump. `scope` 가 옵션 (없으면 전체).
/// Secret 은 export 하지 않는다. 응답: `{ entries: [...], count: N }`.
pub fn handle_export(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
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
    let result = with_store(|s| s.export_regular(scope.as_ref())).expect("memory store present");
    match result {
        Ok(entries) => {
            let arr: Vec<Value> = entries.iter().map(entry_to_json).collect();
            JsonRpcResponse::success(id, json!({ "entries": arr, "count": arr.len() }))
        }
        Err(e) => map_error(id, e),
    }
}

/// `memory.import` — regular 영역으로 entry 입력. `entries` 는 export 응답과 같은
/// 형태의 배열. `replace` (기본 false) 면 기존 key 덮어쓰기. CAS 는 적용하지 않는다.
/// 응답: `{ applied: N, skipped: M }`.
pub fn handle_import(
    _state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if let Err(e) = require_store(&id) {
        return e;
    }
    let entries_json = match params.get("entries").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'entries' array parameter");
        }
    };
    let replace = params
        .get("replace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut entries: Vec<MemoryEntry> = Vec::with_capacity(entries_json.len());
    for ev in &entries_json {
        match parse_export_entry(ev) {
            Ok(e) => entries.push(e),
            Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
        }
    }
    let owner = caller.owner().to_string();
    let result =
        with_store(|s| s.import_regular(&owner, &entries, replace)).expect("memory store present");
    match result {
        Ok(stats) => JsonRpcResponse::success(
            id,
            json!({ "applied": stats.applied, "skipped": stats.skipped }),
        ),
        Err(e) => map_error(id, e),
    }
}

/// export 응답 객체 (`entry_to_json` 출력) 을 `MemoryEntry` 로 복원. timestamp/version
/// 은 import 에서 무시되며 새 record 가 caller_owner 로 발급된다.
fn parse_export_entry(ev: &Value) -> Result<MemoryEntry, String> {
    let scope = ev
        .get("scope")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "entry missing 'scope'".to_string())?
        .to_string();
    let key = ev
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "entry missing 'key'".to_string())?
        .to_string();
    let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("json");
    let value = match kind {
        "text" => match ev.get("value").and_then(|v| v.as_str()) {
            Some(s) => MemoryValue::Text(s.to_string()),
            None => return Err(format!("entry '{key}': kind=text requires 'value' string")),
        },
        "json" => match ev.get("value") {
            Some(v) => MemoryValue::Json(v.clone()),
            None => return Err(format!("entry '{key}': kind=json requires 'value'")),
        },
        "binary" => match ev.get("value_b64").and_then(|v| v.as_str()) {
            Some(b64) => {
                MemoryValue::Binary(decode_b64(b64).map_err(|e| format!("entry '{key}': {e}"))?)
            }
            None => return Err(format!("entry '{key}': kind=binary requires 'value_b64'")),
        },
        other => return Err(format!("entry '{key}': unknown kind '{other}'")),
    };
    let expires_at = ev.get("expires_at").and_then(|v| v.as_i64());
    Ok(MemoryEntry {
        scope,
        key,
        value,
        created_at: 0,
        updated_at: 0,
        expires_at,
        version: 0,
        owner: None,
    })
}

// ============================================================
// Secret `memory.secret.*` — owner 자동 분기, 응답에 owner 미포함
// ============================================================
