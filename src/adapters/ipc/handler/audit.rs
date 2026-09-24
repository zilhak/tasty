//! 감사 기록 조회·집계·삭제. dispatcher의 `local_only` 검사로 로컬 호출만 허용한다.

use super::params::{self, p_try};
use serde_json::{Value, json};

use crate::store::audit::{
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
    JsonRpcResponse::error(id, -32603, err.to_string())
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
    if let Some(n) = params::opt_int::<u64>(params, "since_ms", id)? {
        q.since_ms = Some(n);
    }
    if let Some(n) = params::opt_int::<u64>(params, "until_ms", id)? {
        q.until_ms = Some(n);
    }
    if let Some(n) = params::opt_int::<u64>(params, "limit", id)? {
        q.limit = Some(n as usize);
    }
    Ok(q)
}

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

/// `top_n`은 호출자별·메서드별 상위 항목 수를 제한한다. 기본값은 10이다.
pub fn handle_summary(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let q = match build_query(params, &id) {
        Ok(q) => q,
        Err(resp) => return resp,
    };
    let top_n = p_try!(params::opt_int::<u64>(params, "top_n", &id))
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

/// 커서 이후의 기록을 반환한다. 커서가 없으면 빈 목록과 최신 커서를 반환해
/// 다음 호출부터 새 기록을 조회하게 한다.
#[cfg(feature = "gui")]
pub fn handle_follow(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let q = match build_query(params, &id) {
        Ok(q) => q,
        Err(resp) => return resp,
    };
    let after_ts_ms = p_try!(params::opt_int::<u64>(params, "after_ts_ms", &id));
    let after_seq = p_try!(params::opt_int::<u64>(params, "after_seq", &id));
    let limit = p_try!(params::opt_int::<u64>(params, "limit", &id)).map(|n| n as usize);
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

/// `before_ms` 이전 기록을 삭제한다. 생략하면 모두 삭제한다.
#[cfg(feature = "gui")]
pub fn handle_clear(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let before_ms = p_try!(params::opt_int::<u64>(params, "before_ms", &id));
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
