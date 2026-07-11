//! `webhook.*` IPC 핸들러 — 인바운드 웹훅 등록/조회/해제.
//!
//! **불가침 원칙 2·3**: 웹훅 CRUD 는 에이전트 작업이라 IPC+CLI 양면 노출. 대상은
//! opaque id 로 직접 지정하고 list 는 전 범위 조회 — 사용자 포커스/상태에 부수효과
//! 없음. 웹훅에는 소유자/격리 개념이 없다(전체 목록 공개).
//!
//! 상태는 `crate::webhook` 전역 싱글턴 + `crate::hook_handler` 전역 레지스트리에
//! 있으므로 engine/state 를 받지 않는다.
//!
//! ## 불변식 게이트 (등록 경로)
//! - **source 게이트**: 핸들러 id 로 바인딩 시 `validate_binding(handler, Webhook)` 로
//!   검증 → `source: hook` 전용·`ShellCommand` 는 여기서 거부(`invalid_params`).
//! - **데이터/흐름 분리**: 인라인 시퀀스의 `method` 는 owner(로컬 CLI/IPC)가 준
//!   리터럴이며, 외부 HTTP 페이로드는 이 등록 경로에 닿지 않는다.

use serde_json::json;

use crate::hook_handler::{
    self, HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource, IpcCall,
    TriggerSource, validate_binding,
};
use crate::webhook::lifetime::now_unix;
use crate::webhook::{self, AuthLocation, Lifetime, Limit, Persistence, WebhookAuth, auth_summary};
use tasty_ipc::protocol::JsonRpcResponse;

/// 기본 허용 메서드(요청에 `methods` 미지정 시).
const DEFAULT_METHODS: &[&str] = &["POST"];

/// `webhook.register` — 웹훅 등록 후 발급 URL 반환.
///
/// params:
/// - `methods`: `["POST", ...]` (선택, 기본 `["POST"]`) — 허용 HTTP 메서드.
/// - `handler`: 등록된 훅 핸들러 id (선택) — source 게이트 검증 후 그 IpcSequence 사용.
/// - `sequence`: 인라인 IpcCall 배열 (선택) — owner 가 직접 정의(익명 웹훅 핸들러).
/// - `auth`: 선택적 인증 `{ location, key?, token }` — 미지정 시 무인증 통과.
///
/// `handler` 와 `sequence` 중 정확히 하나를 지정한다.
///
/// lifetime params (선택):
/// - `persistent`: bool (기본 false → Temporary). true 면 재시작 후에도 복원.
/// - `ttl_secs`: u64 — 시간제한(now+ttl 절대 deadline). `count` 와 상호배타.
/// - `count`: u64 — 횟수제한(잔여 호출 수). `ttl_secs` 와 상호배타.
/// - 둘 다 없으면 무제한.
pub fn handle_register(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let methods = parse_methods(params);
    if methods.is_empty() {
        return JsonRpcResponse::invalid_params(id, "'methods' must be a non-empty string array");
    }

    let lifetime = match parse_lifetime(params) {
        Ok(lt) => lt,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };

    // 인증 설정 파싱(선택). 형식 오류는 등록 전 거부.
    let auth = match parse_auth(params) {
        Ok(auth) => auth,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    };

    // CLI 는 미지정 필드를 JSON null 로 보내므로 null 을 "부재" 로 취급한다.
    let handler_field = params.get("handler").and_then(|v| v.as_str());
    let sequence_field = params.get("sequence").filter(|v| !v.is_null());

    let (handler_id, calls) = match (handler_field, sequence_field) {
        (Some(_), Some(_)) => {
            return JsonRpcResponse::invalid_params(
                id,
                "specify exactly one of 'handler' or 'sequence', not both",
            );
        }
        // ── 등록된 핸들러 id 로 바인딩 ──
        (Some(hid), None) => {
            let hid = HookHandlerId::new(hid);
            let reg = hook_handler::global();
            let Some(handler) = reg.get(&hid) else {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("hook handler '{hid}' not found"),
                );
            };
            // source 게이트 — 셸/hook-전용 핸들러는 여기서 거부(불변식).
            if let Err(e) = validate_binding(handler, TriggerSource::Webhook) {
                return JsonRpcResponse::invalid_params(id, e.to_string());
            }
            match &handler.action {
                HookHandlerAction::IpcSequence { calls } => (Some(hid.clone()), calls.clone()),
                // is_webhook_bindable 이 이미 걸러내지만 방어적 이중 확인.
                HookHandlerAction::ShellCommand { .. } => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("hook handler '{hid}' is a shell command; not webhook-bindable"),
                    );
                }
            }
        }
        // ── 인라인 시퀀스 (owner 직접 정의 → 익명 웹훅 핸들러 등록) ──
        (None, Some(seq)) => match serde_json::from_value::<Vec<IpcCall>>(seq.clone()) {
            Ok(calls) if !calls.is_empty() => {
                let anon = anonymous_handler(&calls);
                let anon_id = anon.id.clone();
                // 익명 핸들러를 레지스트리에도 반영(조회/일관성). 실패해도 웹훅
                // 등록은 진행(calls 스냅샷을 웹훅 엔트리가 이미 소유).
                if let Err(e) = hook_handler::global().upsert(anon) {
                    tracing::warn!("anonymous hook handler upsert failed: {e}");
                }
                (Some(anon_id), calls)
            }
            Ok(_) => {
                return JsonRpcResponse::invalid_params(id, "'sequence' must be non-empty");
            }
            Err(e) => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("'sequence' must be an array of {{method, params}}: {e}"),
                );
            }
        },
        (None, None) => {
            return JsonRpcResponse::invalid_params(id, "provide 'handler' or 'sequence'");
        }
    };

    let auth_json = auth.as_ref().map(auth_summary);
    let outcome = webhook::register(methods.clone(), handler_id.clone(), calls, lifetime, auth);
    JsonRpcResponse::success(
        id,
        json!({
            "id": outcome.id,
            "url": outcome.url,
            "methods": methods,
            "handler": handler_id.map(|h| h.0),
            "lifetime": lifetime_json(&lifetime),
            "auth": auth_json,
        }),
    )
}

/// `webhook.sweep` — 만료된(시간 초과 / 횟수 소진) 웹훅 일괄 정리. 제거된 id 목록 반환.
pub fn handle_sweep(id: serde_json::Value) -> JsonRpcResponse {
    let swept = webhook::sweep();
    JsonRpcResponse::success(id, json!({ "swept": swept, "count": swept.len() }))
}

/// lifetime params 를 파싱한다. `ttl_secs`/`count` 는 상호배타.
fn parse_lifetime(params: &serde_json::Value) -> Result<Lifetime, String> {
    let persistence = if params
        .get("persistent")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        Persistence::Persistent
    } else {
        Persistence::Temporary
    };

    // CLI 는 미지정 optional 을 JSON null 로 보내므로 null 은 "부재" 로 취급한다.
    let ttl = params.get("ttl_secs").filter(|v| !v.is_null());
    let count = params.get("count").filter(|v| !v.is_null());

    let limit = match (ttl, count) {
        (Some(_), Some(_)) => {
            return Err("specify at most one of 'ttl_secs' or 'count', not both".to_string());
        }
        (Some(ttl), None) => {
            let secs = ttl
                .as_u64()
                .ok_or_else(|| "'ttl_secs' must be a positive integer".to_string())?;
            if secs == 0 {
                return Err("'ttl_secs' must be greater than 0".to_string());
            }
            Limit::TimeLimit {
                deadline_unix: now_unix().saturating_add(secs),
            }
        }
        (None, Some(count)) => {
            let remaining = count
                .as_u64()
                .ok_or_else(|| "'count' must be a positive integer".to_string())?;
            if remaining == 0 {
                return Err("'count' must be greater than 0".to_string());
            }
            Limit::CountLimit { remaining }
        }
        (None, None) => Limit::Unlimited,
    };

    Ok(Lifetime { persistence, limit })
}

/// lifetime → JSON(등록/조회 응답, 로컬 owner 채널). 시간제한은 절대 시각 +
/// 현재 기준 잔여 초를, 횟수제한은 남은 카운트를 노출한다.
fn lifetime_json(lifetime: &Lifetime) -> serde_json::Value {
    let persistence = match lifetime.persistence {
        Persistence::Persistent => "persistent",
        Persistence::Temporary => "temporary",
    };
    match lifetime.limit {
        Limit::Unlimited => json!({ "persistence": persistence, "limit": "unlimited" }),
        Limit::TimeLimit { deadline_unix } => {
            let now = now_unix();
            json!({
                "persistence": persistence,
                "limit": "time",
                "expires_at_unix": deadline_unix,
                "expires_in_secs": deadline_unix.saturating_sub(now),
                "expired": now >= deadline_unix,
            })
        }
        Limit::CountLimit { remaining } => json!({
            "persistence": persistence,
            "limit": "count",
            "remaining": remaining,
        }),
    }
}

/// `webhook.list` — 전체 웹훅 목록(포커스 독립, 전 범위).
pub fn handle_list(id: serde_json::Value) -> JsonRpcResponse {
    let items: Vec<_> = webhook::list()
        .into_iter()
        .map(|(e, url)| entry_json(&e, &url))
        .collect();
    JsonRpcResponse::success(id, json!({ "webhooks": items }))
}

/// `webhook.info` — 단일 웹훅 상세(id 지정).
pub fn handle_info(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(wid) = params.get("id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' parameter");
    };
    match webhook::info(wid) {
        Some((e, url)) => JsonRpcResponse::success(id, entry_json(&e, &url)),
        None => JsonRpcResponse::invalid_params(id, format!("webhook '{wid}' not found")),
    }
}

/// `webhook.unregister` — 웹훅 해제. path 회수 → 이후 그 URL 은 404.
pub fn handle_unregister(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(wid) = params.get("id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' parameter");
    };
    let removed = webhook::unregister(wid);
    JsonRpcResponse::success(id, json!({ "unregistered": removed, "id": wid }))
}

/// `methods` 파라미터 파싱 — 대문자 정규화. 미지정 시 기본값.
fn parse_methods(params: &serde_json::Value) -> Vec<String> {
    match params.get("methods") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_ascii_uppercase())
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.to_ascii_uppercase()],
        _ => DEFAULT_METHODS.iter().map(|s| s.to_string()).collect(),
    }
}

/// `auth` 파라미터 파싱(선택). 미지정/`null` 이면 `Ok(None)`(무인증).
///
/// 형식: `{ "location": "query"|"bearer"|"body"|"header", "key": "<name>",
/// "token": "<secret>" }`. `bearer` 는 `key` 불요, 나머지는 필수. `token` 은 항상 필수.
fn parse_auth(params: &serde_json::Value) -> Result<Option<WebhookAuth>, String> {
    let Some(auth) = params.get("auth").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let obj = auth
        .as_object()
        .ok_or_else(|| "'auth' must be an object".to_string())?;

    let token = obj
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "'auth.token' is required and must be a non-empty string".to_string())?
        .to_string();

    let location_kind = obj
        .get("location")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "'auth.location' is required".to_string())?;
    let key = obj.get("key").and_then(|v| v.as_str());

    let require_key = |what: &str| -> Result<String, String> {
        key.filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("'auth.key' is required for {what} auth location"))
    };

    let location = match location_kind {
        "query" => AuthLocation::QueryKey { key: require_key("query")? },
        "bearer" => AuthLocation::BearerHeader,
        "body" => AuthLocation::BodyField { field: require_key("body")? },
        "header" => AuthLocation::HeaderKey { name: require_key("header")? },
        other => {
            return Err(format!(
                "'auth.location' must be one of query|bearer|body|header, got '{other}'"
            ));
        }
    };

    Ok(Some(WebhookAuth { location, token }))
}

/// 인라인 시퀀스로부터 익명 웹훅 핸들러 생성 — 결정적 id(첫 method 기반 + 카운트).
fn anonymous_handler(calls: &[IpcCall]) -> HookHandler {
    let first = calls.first().map(|c| c.method.as_str()).unwrap_or("seq");
    // short-name 규약: [a-z0-9-]{1,32}. method 의 '.' 를 '-' 로.
    let slug: String = first
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .take(24)
        .collect();
    HookHandler {
        id: HookHandlerId::new(format!("user/wh-{slug}")),
        source: HookSource::Webhook,
        priority: 0,
        owner: HookHandlerOwner::User,
        action: HookHandlerAction::IpcSequence {
            calls: calls.to_vec(),
        },
        display_name_i18n_key: None,
        disabled: false,
    }
}

/// 웹훅 엔트리 → JSON(조회 응답). 발급 URL·메서드·핸들러·lifetime(남은횟수/만료)
/// 노출(로컬 owner 채널). 인증은 **위치/키만** 요약 노출하고 **토큰은 절대 싣지
/// 않는다**(`auth_summary`).
fn entry_json(e: &webhook::WebhookEntry, url: &str) -> serde_json::Value {
    json!({
        "id": e.id,
        "url": url,
        "methods": e.methods,
        "handler": e.handler_id.as_ref().map(|h| h.0.clone()),
        "steps": e.calls.len(),
        "lifetime": lifetime_json(&e.lifetime),
        "auth": e.auth.as_ref().map(auth_summary),
    })
}
