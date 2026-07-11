//! `hook_handler.*` IPC 핸들러 — 공유 훅 핸들러 레지스트리 조회/재로드/수동 발화.
//!
//! 파일 핸들러(`file_handler.*`)를 선례로 미러링한다. 상태는 `crate::hook_handler`
//! 전역 싱글턴 레지스트리라 engine/state 를 받지 않는다(단 dispatch 는 IpcSequence
//! 실행에 host injector 가 필요해 `core` 를 받는다).
//!
//! **불가침 원칙 2·3**: 핸들러 조회/발화는 에이전트 작업이라 IPC+CLI 양면 노출.
//! 대상은 id 로 직접 지정하고 list 는 전 범위(비활성 포함) 조회 — 사용자 포커스/
//! 상태에 부수효과 없음.
//!
//! ## 불변식
//! - **데이터/흐름 분리**: 발화되는 `IpcCall.method` 는 owner 가 등록 시 고정한
//!   리터럴이며 dispatch 페이로드는 `params` 값 슬롯에만 치환된다([`crate::hook_handler::exec`]).
//! - **단방향(fire-and-forget)**: dispatch 실행 결과는 worker thread 안에 갇히고
//!   JSON 응답에 실행 결과가 실리지 않는다(응답은 "수리됨" ACK 만).

use serde_json::json;

use crate::core::Core;
use crate::hook_handler::{
    self, HookHandlerAction, HookHandlerId, SubstitutionContext, execute_sequence, spawn_shell,
};
use tasty_ipc::protocol::JsonRpcResponse;

/// `hook_handler.list` — 등록된 모든 훅 핸들러(비활성 포함, 포커스 독립·전 범위).
///
/// 각 항목: id / source / priority / owner / action kind(+steps) / disabled /
/// display_name_i18n_key / webhook_bindable. action 의 실제 IPC 호출 목록·셸 명령은
/// 노출하지 않는다(요약만).
pub fn handle_list(id: serde_json::Value) -> JsonRpcResponse {
    let items: Vec<_> = hook_handler::global()
        .all_handlers_including_disabled()
        .into_iter()
        .map(|h| {
            let (action_kind, steps) = match &h.action {
                HookHandlerAction::IpcSequence { calls } => ("ipc_sequence", Some(calls.len())),
                HookHandlerAction::ShellCommand { .. } => ("shell_command", None),
            };
            json!({
                "id": h.id.0,
                "source": h.source,
                "priority": h.priority,
                "owner": h.owner.prefix(),
                "action": action_kind,
                "steps": steps,
                "disabled": h.disabled,
                "display_name_i18n_key": h.display_name_i18n_key,
                "webhook_bindable": h.action.is_webhook_bindable(),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "handlers": items }))
}

/// `hook_handler.reload` — `~/.tasty/hook-handlers.toml`(user 출처) 재로드.
///
/// host embedded 기본값 + plugin contribution 은 영향받지 않는다(user 출처만 교체).
/// 파일 핸들러 `file_handler.reload` 응답 형태(`{path, exists}`)를 미러링한다.
pub fn handle_reload(id: serde_json::Value) -> JsonRpcResponse {
    let Some(path) = hook_handler::user_config_path() else {
        return JsonRpcResponse::internal_error(id, "cannot resolve tasty home for hook-handlers.toml");
    };
    let exists = path.exists();
    hook_handler::global().reload_user_config(&path);
    JsonRpcResponse::success(
        id,
        json!({
            "path": path.display().to_string(),
            "exists": exists,
        }),
    )
}

/// `hook_handler.dispatch` — 등록된 핸들러를 id 로 **수동 발화**한다.
///
/// 파일 핸들러 `file_handler.dispatch`(임의 경로를 dispatch 흐름에 진입)의 훅 핸들러
/// 대응물 — 에이전트/CLI 가 트리거 없이 핸들러를 즉시 실행하는 진입점(테스트·자동화).
///
/// params:
/// - `id`: 발화할 핸들러 id (필수).
/// - `body` / `headers` / `query`: 치환 컨텍스트(선택). IpcSequence 핸들러의 `params`
///   값 슬롯(`${body.x}`/`${header.x}`/`${query.x}`)에 채워진다.
///
/// 응답은 "수리됨(accepted)" ACK 만 담는다 — 실행은 worker thread 에서 fire-and-forget
/// 되고 결과가 응답에 실리지 않는다(단방향 불변식). 비활성 핸들러는 거부한다.
pub fn handle_dispatch(
    core: &Core,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(hid) = params.get("id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' parameter");
    };
    let hid = HookHandlerId::new(hid);
    let Some(handler) = hook_handler::global().get(&hid) else {
        return JsonRpcResponse::invalid_params(id, format!("hook handler '{hid}' not found"));
    };
    if handler.disabled {
        return JsonRpcResponse::invalid_params(id, format!("hook handler '{hid}' is disabled"));
    }

    match handler.action {
        HookHandlerAction::IpcSequence { calls } => {
            // injector 를 얻어 worker thread 에서 실행한다. 메인 스레드(현재 핸들러)를
            // 막지 않아야 하므로 dispatch 를 spawn 뒤로 넘긴다(웹훅 리스너와 동일 패턴).
            let Some(injector) = core.host_ipc_injector_arc().get().cloned() else {
                return JsonRpcResponse::internal_error(
                    id,
                    "host IPC injector not initialized (dispatch unavailable before boot completes)",
                );
            };
            let ctx = build_context(params);
            let steps = calls.len();
            if let Err(e) = std::thread::Builder::new()
                .name("hook-dispatch".into())
                .spawn(move || execute_sequence(&injector, &calls, &ctx))
            {
                return JsonRpcResponse::internal_error(id, format!("dispatch thread spawn: {e}"));
            }
            JsonRpcResponse::success(
                id,
                json!({ "accepted": true, "id": hid.0, "action": "ipc_sequence", "steps": steps }),
            )
        }
        HookHandlerAction::ShellCommand { command, args } => {
            spawn_shell(command, args);
            JsonRpcResponse::success(
                id,
                json!({ "accepted": true, "id": hid.0, "action": "shell_command" }),
            )
        }
    }
}

/// dispatch 치환 컨텍스트 조립. body 는 JSON 값 그대로, headers/query 는 문자열 맵.
fn build_context(params: &serde_json::Value) -> SubstitutionContext {
    let body = params.get("body").cloned().unwrap_or(serde_json::Value::Null);
    let headers = string_map(params.get("headers"), true);
    let query = string_map(params.get("query"), false);
    SubstitutionContext {
        body,
        headers,
        query,
    }
}

/// JSON object → `BTreeMap<String,String>`. `lowercase_keys` 면 헤더처럼 키를 소문자
/// 정규화한다(exec 의 헤더 조회 규약과 일치). 값이 문자열이 아니면 JSON 표현으로.
fn string_map(
    v: Option<&serde_json::Value>,
    lowercase_keys: bool,
) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    if let Some(serde_json::Value::Object(obj)) = v {
        for (k, val) in obj {
            let key = if lowercase_keys {
                k.to_ascii_lowercase()
            } else {
                k.clone()
            };
            let value = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            map.insert(key, value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn string_map_lowercases_header_keys() {
        let m = string_map(Some(&json!({"X-Sig": "abc", "Y": 1})), true);
        assert_eq!(m.get("x-sig"), Some(&"abc".to_string()));
        assert_eq!(m.get("y"), Some(&"1".to_string()));
    }

    #[test]
    fn string_map_preserves_query_keys() {
        let m = string_map(Some(&json!({"Token": "t"})), false);
        assert_eq!(m.get("Token"), Some(&"t".to_string()));
    }

    #[test]
    fn build_context_defaults_null_body() {
        let ctx = build_context(&json!({}));
        assert_eq!(ctx.body, serde_json::Value::Null);
        assert!(ctx.headers.is_empty());
    }
}
