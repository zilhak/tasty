//! `session.*` IPC 핸들러 — 자식 agent 에게 발급하는 [`SessionToken`] 관리.
//!
//! Phase 6.2c. 호스트가 띄운 child 프로세스(예: `claude.spawn`)는 시작 시
//! 환경변수 `TASTY_SESSION_TOKEN` 으로 토큰을 받아 모든 IPC envelope 에 첨부한다.
//! 호스트는 [`crate::ipc::session::SessionStore`] 로 검증해
//! [`CallerContext::Agent`] 로 분기 — agent_id 위조 방지의 핵심.
//!
//! 권한 모델:
//! - `session.issue` 는 `AgentManage` 필요. 호출자(부모) 가 자식에게 권한을
//!   넘긴다. 호스트는 자식에게 **자신이 가진 권한의 부분집합만** 발급할 수
//!   있다 (escalation 방지). Local caller 는 무제한.
//! - `session.revoke` 는 `AgentManage` 필요. 임의 토큰을 무효화 — 부모-자식
//!   확인은 하지 않는다 (어차피 AgentManage 가 있으면 새 token 을 만들 수
//!   있으므로 추가 게이트가 의미 없음).
//! - `session.list` 는 host 전용 (`local_only`). 모든 활성 세션을 본다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::session::{SessionError, SessionStore};
use crate::plugin::manifest::Permission;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn session_err_to_response(id: Value, err: SessionError) -> JsonRpcResponse {
    match err {
        SessionError::InvalidArgument(_) => JsonRpcResponse::invalid_params(id, err.to_string()),
        SessionError::Memory(_) | SessionError::Serde(_) => {
            JsonRpcResponse::error(id, -32603, &err.to_string())
        }
    }
}

/// `session.issue` — 자식 agent 에게 새 SessionToken 발급.
///
/// params:
/// - `agent_id` (str, 필수) — 자식의 호스트-부여 식별자. 비어 있으면 거부.
/// - `permissions` (array<str>, 옵션) — 자식에게 부여할 권한 토큰. caller 의
///   권한 셋에 포함되지 않은 토큰이 있으면 거부 (escalation 방지). Local
///   caller 는 모든 토큰 허용.
/// - `ttl_ms` (u64, 옵션) — 토큰 수명. 없으면 자식 프로세스 종료/`session.revoke`
///   까지 유효.
///
/// 응답: `{ token, agent_id, expires_at_ms? }`.
pub fn handle_issue(
    core: &crate::core::Core,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return JsonRpcResponse::invalid_params(id, "Missing or empty 'agent_id'");
        }
    };
    let perm_tokens: Vec<String> = match params.get("permissions") {
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => {
                        return JsonRpcResponse::invalid_params(
                            id,
                            "'permissions' must be an array of strings",
                        );
                    }
                }
            }
            out
        }
        Some(Value::Null) | None => Vec::new(),
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "'permissions' must be an array of strings",
            );
        }
    };
    let ttl_ms = params.get("ttl_ms").and_then(|v| v.as_u64());

    // 권한 토큰을 Permission 으로 매핑하고, 알 수 없는 토큰을 거부.
    let mut perms: Vec<Permission> = Vec::with_capacity(perm_tokens.len());
    for t in &perm_tokens {
        match Permission::from_token(t) {
            Some(p) => perms.push(p),
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("unknown permission token: {t}"),
                );
            }
        }
    }

    // Escalation 방지: caller 가 가진 권한의 부분집합만 발급 가능.
    // Local/Internal 은 무제한. Plugin/Agent 는 자기 권한 셋을 기준으로 검사.
    if let Some(caller_perms) = caller.permissions() {
        for p in &perms {
            if !caller_perms.contains(p) {
                return JsonRpcResponse::error(
                    id,
                    -32001,
                    &format!(
                        "caller cannot grant permission '{}' (not in own permissions)",
                        p.as_token()
                    ),
                );
            }
        }
    }

    let parent = match caller {
        CallerContext::Local => None,
        CallerContext::Plugin { plugin_id, .. } => Some(plugin_id.clone()),
        CallerContext::Agent { agent_id, .. } => Some(agent_id.clone()),
    };

    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.issue(agent_id.clone(), parent.clone(), perms.clone(), ttl_ms, now)
    });
    match result {
        Ok((token, session)) => JsonRpcResponse::success(
            id,
            json!({
                "token": token.as_str(),
                "agent_id": session.agent_id,
                "parent": session.parent,
                "expires_at_ms": session.expires_at_ms,
            }),
        ),
        Err(e) => session_err_to_response(id, e),
    }
}

/// `session.revoke` — 주어진 토큰 무효화.
///
/// params: `{ token: str }`. 응답: `{ revoked: bool }` (없으면 false).
pub fn handle_revoke(core: &crate::core::Core, id: Value, params: &Value) -> JsonRpcResponse {
    let token_str = match params.get("token").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'token'");
        }
    };
    let token = match crate::ipc::caller::SessionToken::from_str(&token_str) {
        Some(t) => t,
        None => {
            return JsonRpcResponse::invalid_params(id, "Invalid 'token' (must be 64 hex chars)");
        }
    };
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.revoke(&token)
    });
    match result {
        Ok(revoked) => JsonRpcResponse::success(id, json!({ "revoked": revoked })),
        Err(e) => session_err_to_response(id, e),
    }
}

/// `session.list` — 활성 세션 목록 (host 전용, 디버깅/감사용).
pub fn handle_list(core: &crate::core::Core, id: Value) -> JsonRpcResponse {
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.list(now)
    });
    match result {
        Ok(sessions) => {
            let arr: Vec<Value> = sessions
                .into_iter()
                .map(|s| {
                    json!({
                        "agent_id": s.agent_id,
                        "parent": s.parent,
                        "permissions": s.permissions,
                        "temp_grants": s.temp_grants.iter().map(|g| json!({
                            "permission": g.permission,
                            "expires_at_ms": g.expires_at_ms,
                        })).collect::<Vec<_>>(),
                        "created_at_ms": s.created_at_ms,
                        "expires_at_ms": s.expires_at_ms,
                    })
                })
                .collect();
            JsonRpcResponse::success(id, json!({ "sessions": arr }))
        }
        Err(e) => session_err_to_response(id, e),
    }
}

/// `plugin.grant_agent_permission` — 활성 agent 에게 임시 권한 grant.
///
/// params:
/// - `agent_id` (str, 필수) — 대상 agent.
/// - `permission` (str, 필수) — 권한 토큰. 알 수 없는 토큰이면 거부.
/// - `ttl_secs` (u64, 옵션) — 만료까지 초. 없으면 무기한 (revoke 까지 유효).
///
/// 동일 token 이 base 에 이미 있으면 noop. 중복 grant 는 만료 시점을 갱신.
/// 응답: `{ agent_id, permission, added, expires_at_ms? }`.
pub fn handle_grant_agent_permission(
    core: &crate::core::Core,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return JsonRpcResponse::invalid_params(id, "Missing or empty 'agent_id'");
        }
    };
    let permission = match params.get("permission").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return JsonRpcResponse::invalid_params(id, "Missing or empty 'permission'");
        }
    };
    if Permission::from_token(&permission).is_none() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("unknown permission token: {permission}"),
        );
    }
    let ttl_ms = params
        .get("ttl_secs")
        .and_then(|v| v.as_u64())
        .map(|s| s.saturating_mul(1000));
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        let (token, _session) = match store.find_by_agent_id(&agent_id, now)? {
            Some(t) => t,
            None => {
                return Err(SessionError::InvalidArgument(format!(
                    "no active session for agent_id '{agent_id}'"
                )));
            }
        };
        let added = store.grant_permission(&token, &permission, ttl_ms, now)?;
        let expires_at = ttl_ms.map(|t| now.saturating_add(t));
        Ok::<_, SessionError>((added, expires_at))
    });
    match result {
        Ok((added, expires_at)) => JsonRpcResponse::success(
            id,
            json!({
                "agent_id": agent_id,
                "permission": permission,
                "added": added,
                "expires_at_ms": expires_at,
            }),
        ),
        Err(e) => session_err_to_response(id, e),
    }
}

/// `plugin.revoke_agent_permission` — 활성 agent 에서 임시 권한 회수.
///
/// params: `{ agent_id, permission }`. base permission 은 건드리지 않는다.
/// 응답: `{ agent_id, permission, removed }`.
pub fn handle_revoke_agent_permission(
    core: &crate::core::Core,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'agent_id'"),
    };
    let permission = match params.get("permission").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'permission'"),
    };
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        let (token, _) = match store.find_by_agent_id(&agent_id, now)? {
            Some(t) => t,
            None => return Ok::<bool, SessionError>(false),
        };
        store.revoke_permission(&token, &permission, now)
    });
    match result {
        Ok(removed) => JsonRpcResponse::success(
            id,
            json!({
                "agent_id": agent_id,
                "permission": permission,
                "removed": removed,
            }),
        ),
        Err(e) => session_err_to_response(id, e),
    }
}

/// `plugin.list_agent_permissions` — 활성 agent 의 base + temp permission 조회.
///
/// params:
/// - `agent_id` (str, 옵션) — 지정 시 해당 agent 만, 없으면 모든 활성 세션.
///
/// 응답: `{ agents: [{ agent_id, parent, base_permissions, temp_grants: [{permission, expires_at_ms?}] }] }`.
pub fn handle_list_agent_permissions(
    core: &crate::core::Core,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let target_id = params
        .get("agent_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let now = now_ms();
    let result = core.with_memory(|mem| {
        let mut store = SessionStore::new(mem, tasty_memory::HOST_OWNER);
        store.list(now)
    });
    match result {
        Ok(sessions) => {
            let arr: Vec<Value> = sessions
                .into_iter()
                .filter(|s| match &target_id {
                    Some(want) => &s.agent_id == want,
                    None => true,
                })
                .map(|s| {
                    json!({
                        "agent_id": s.agent_id,
                        "parent": s.parent,
                        "base_permissions": s.permissions,
                        "temp_grants": s.temp_grants.iter().map(|g| json!({
                            "permission": g.permission,
                            "expires_at_ms": g.expires_at_ms,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            JsonRpcResponse::success(id, json!({ "agents": arr }))
        }
        Err(e) => session_err_to_response(id, e),
    }
}

/// Phase 6.4c — `plugin.request_permission` 핸들러. agent 가 자기 권한 부족을
/// 미리 알고 capability_elevation approval 을 자체 발행할 entry point.
///
/// params:
/// - `agent_id` (str) — 대상. Agent caller 면 미지정 시 caller 자신의 agent_id
///   로 기본값. Plugin/Local caller 는 필수 (운영자가 대신 발행).
/// - `permission` (str, 필수) — 요청 권한 토큰.
/// - `reason` (str, 옵션) — 사용자에게 보여줄 사유.
///
/// 응답: `{ approval_id }`. dedupe 로직 (같은 agent+permission Pending 재사용)
/// 은 publish_capability_elevation 안에서 처리.
pub fn handle_request_permission(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut crate::engine_state::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let agent_id = params
        .get("agent_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| match caller {
            CallerContext::Agent { agent_id, .. } => Some(agent_id.clone()),
            _ => None,
        });
    let agent_id = match agent_id {
        Some(s) => s,
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing 'agent_id' (required for non-agent callers)",
            );
        }
    };
    let permission = match params.get("permission").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'permission'"),
    };
    if Permission::from_token(&permission).is_none() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("unknown permission token: {permission}"),
        );
    }
    let reason = params
        .get("reason")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let method = params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("(self-request)");
    match crate::ipc::handler::approval::publish_capability_elevation(
        core,
        state,
        engine,
        &agent_id,
        method,
        &permission,
        reason,
    ) {
        Some(rec) => JsonRpcResponse::success(
            id,
            json!({
                "approval_id": rec.request.id,
                "agent_id": agent_id,
                "permission": permission,
            }),
        ),
        None => JsonRpcResponse::error(id, -32603, "elevation publish failed"),
    }
}

// 옛 handler validation 단위 테스트 모듈은 D.3.C.M.14 에서 제거.
// 핸들러 시그니처에 `&Core` 가 들어가면서 mock 비용이 크고, 핵심인 영속 통합은
// `crate::ipc::session::tests` 가 SessionStore 직접 호출로 이미 검증한다.
