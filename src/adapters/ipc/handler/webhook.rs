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
use crate::webhook;
use tasty_ipc::protocol::JsonRpcResponse;

/// 기본 허용 메서드(요청에 `methods` 미지정 시).
const DEFAULT_METHODS: &[&str] = &["POST"];

/// `webhook.register` — 웹훅 등록 후 발급 URL 반환.
///
/// params:
/// - `methods`: `["POST", ...]` (선택, 기본 `["POST"]`) — 허용 HTTP 메서드.
/// - `handler`: 등록된 훅 핸들러 id (선택) — source 게이트 검증 후 그 IpcSequence 사용.
/// - `sequence`: 인라인 IpcCall 배열 (선택) — owner 가 직접 정의(익명 웹훅 핸들러).
///
/// `handler` 와 `sequence` 중 정확히 하나를 지정한다.
pub fn handle_register(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let methods = parse_methods(params);
    if methods.is_empty() {
        return JsonRpcResponse::invalid_params(id, "'methods' must be a non-empty string array");
    }

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

    // lifetime 파싱은 후속 배선(commit B)에서 params 로 확장한다. 현재는 기존
    // MVP 동작 유지(임시·무제한).
    let lifetime = webhook::Lifetime::temporary_unlimited();
    let outcome = webhook::register(methods.clone(), handler_id.clone(), calls, lifetime);
    JsonRpcResponse::success(
        id,
        json!({
            "id": outcome.id,
            "url": outcome.url,
            "methods": methods,
            "handler": handler_id.map(|h| h.0),
        }),
    )
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

/// 웹훅 엔트리 → JSON(조회 응답). 발급 URL·메서드·핸들러 노출(로컬 owner 채널).
fn entry_json(e: &webhook::WebhookEntry, url: &str) -> serde_json::Value {
    json!({
        "id": e.id,
        "url": url,
        "methods": e.methods,
        "handler": e.handler_id.as_ref().map(|h| h.0.clone()),
        "steps": e.calls.len(),
    })
}
