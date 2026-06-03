//! Phase 6.5b — `plugin.audit_*` IPC 핸들러.
//!
//! audit log 조회/집계/삭제. CallerContext 검사는 method_meta 의 `local_only`
//! 가 dispatcher 레벨에서 거른다 (운영자 전용).

use serde_json::{Value, json};

use crate::ipc::audit::{
    AuditCallerKind, AuditDecision, AuditError, AuditQuery, AuditRecord, AuditStore,
    DEFAULT_RETENTION_MS,
};
use tasty_ipc::protocol::JsonRpcResponse;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn audit_err_to_response(id: Value, err: AuditError) -> JsonRpcResponse {
    JsonRpcResponse::error(id, -32603, &err.to_string())
}

fn parse_caller_kind(s: &str) -> Option<AuditCallerKind> {
    match s {
        "local" => Some(AuditCallerKind::Local),
        "plugin" => Some(AuditCallerKind::Plugin),
        "agent" => Some(AuditCallerKind::Agent),
        _ => None,
    }
}

fn parse_decision(s: &str) -> Option<AuditDecision> {
    match s {
        "allow" => Some(AuditDecision::Allow),
        "deny" => Some(AuditDecision::Deny),
        _ => None,
    }
}

fn record_to_json(r: &AuditRecord) -> Value {
    serde_json::to_value(r).unwrap_or(Value::Null)
}

fn build_query(params: &Value, id: &Value) -> std::result::Result<AuditQuery, JsonRpcResponse> {
    let mut q = AuditQuery::default();
    if let Some(s) = params.get("caller_kind").and_then(|v| v.as_str()) {
        q.caller_kind = Some(parse_caller_kind(s).ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), format!("unknown caller_kind '{s}'"))
        })?);
    }
    if let Some(s) = params
        .get("caller_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        q.caller_id = Some(s.to_string());
    }
    if let Some(s) = params
        .get("method_prefix")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        q.method_prefix = Some(s.to_string());
    }
    if let Some(s) = params.get("decision").and_then(|v| v.as_str()) {
        q.decision = Some(parse_decision(s).ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), format!("unknown decision '{s}'"))
        })?);
    }
    if let Some(n) = params.get("since_ms").and_then(|v| v.as_u64()) {
        q.since_ms = Some(n);
    }
    if let Some(n) = params.get("until_ms").and_then(|v| v.as_u64()) {
        q.until_ms = Some(n);
    }
    if let Some(n) = params.get("limit").and_then(|v| v.as_u64()) {
        q.limit = Some(n as usize);
    }
    Ok(q)
}

/// `plugin.audit_query` — 필터된 audit record 목록.
pub fn handle_query(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let q = match build_query(params, &id) {
        Ok(q) => q,
        Err(resp) => return resp,
    };
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
        store.query(&q, DEFAULT_RETENTION_MS, now)
    });
    match result {
        Ok(records) => {
            let arr: Vec<Value> = records.iter().map(record_to_json).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "records": arr,
                    "count": records.len(),
                }),
            )
        }
        Err(e) => audit_err_to_response(id, e),
    }
}

/// `plugin.audit_summary` — 필터된 record 의 집계.
/// `top_n` (옵션, 기본 10) 으로 by_caller / by_method 상위 개수 제한.
pub fn handle_summary(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let q = match build_query(params, &id) {
        Ok(q) => q,
        Err(resp) => return resp,
    };
    let top_n = params
        .get("top_n")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(10);
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
        store.summary(&q, DEFAULT_RETENTION_MS, now, top_n)
    });
    match result {
        Ok(s) => JsonRpcResponse::success(
            id,
            json!({
                "total": s.total,
                "allow": s.allow,
                "deny": s.deny,
                "by_caller": s.by_caller.into_iter().map(|(k, v)| json!({"caller_id": k, "count": v})).collect::<Vec<_>>(),
                "by_method": s.by_method.into_iter().map(|(k, v)| json!({"method": k, "count": v})).collect::<Vec<_>>(),
            }),
        ),
        Err(e) => audit_err_to_response(id, e),
    }
}

/// `plugin.audit_follow` — `after_ts_ms` / `after_seq` 커서 이후의 새 record.
/// 커서 미지정 시 빈 배열 + 현재 latest 커서를 반환해 호출자가 그 다음부터
/// 폴링하게 한다 (`tail -f -n 0` 시멘틱).
pub fn handle_follow(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let q = match build_query(params, &id) {
        Ok(q) => q,
        Err(resp) => return resp,
    };
    let after_ts_ms = params.get("after_ts_ms").and_then(|v| v.as_u64());
    let after_seq = params.get("after_seq").and_then(|v| v.as_u64());
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
        store.follow(&q, after_ts_ms, after_seq, DEFAULT_RETENTION_MS, now, limit)
    });
    match result {
        Ok((records, next_ts, next_seq)) => {
            let arr: Vec<Value> = records.iter().map(record_to_json).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "records": arr,
                    "count": arr.len(),
                    "next_after_ts_ms": next_ts,
                    "next_after_seq": next_seq,
                }),
            )
        }
        Err(e) => audit_err_to_response(id, e),
    }
}

/// `plugin.audit_clear` — `before_ms` 이전 record 삭제 (생략 시 전체).
/// 반환: `{ removed: N }`.
pub fn handle_clear(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let before_ms = params.get("before_ms").and_then(|v| v.as_u64());
    let result = core.with_memory(|mem| {
        let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
        store.clear(before_ms)
    });
    match result {
        Ok(n) => JsonRpcResponse::success(id, json!({ "removed": n })),
        Err(e) => audit_err_to_response(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // build_query 단독 테스트 — 옛 handle_* 호출 테스트는 Core 의존성이
    // 생기면서 build_query 자체의 validation 만 검사하도록 좁힘.

    fn err_resp_code(resp: &JsonRpcResponse) -> i32 {
        resp.error.as_ref().expect("expected error").code
    }

    #[test]
    fn build_query_rejects_unknown_caller_kind() {
        let id = json!(1);
        let r = build_query(&json!({"caller_kind": "nope"}), &id).unwrap_err();
        assert_eq!(err_resp_code(&r), -32602);
    }

    #[test]
    fn build_query_rejects_unknown_decision() {
        let id = json!(1);
        let r = build_query(&json!({"decision": "maybe"}), &id).unwrap_err();
        assert_eq!(err_resp_code(&r), -32602);
    }
}
