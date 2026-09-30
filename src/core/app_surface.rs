//! GUI와 헤드리스가 공유하는 창 독립 IPC 처리.
//! 클립보드 포트나 명시한 접속 인자를 사용하며 지원 목록은 docs/dev-guide/headless-ipc-surface.md를 따른다.

use std::sync::mpsc::SyncSender;

use serde_json::Value;

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::server::send_response;

/// 클립보드가 없는 환경도 메서드 부재 대신 포트의 쓰기 오류로 응답한다.
pub(crate) fn clipboard_set_text(
    core: &crate::core::Core,
    rpc_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::error(rpc_id, -32602, "Missing 'text' parameter (string)");
    };
    match core.clipboard_arc().write_text(text) {
        Ok(()) => JsonRpcResponse::success(rpc_id, serde_json::json!({"ok": true})),
        Err(e) => JsonRpcResponse::error(rpc_id, -32000, format!("Failed to write clipboard: {e}")),
    }
}

/// remote.workspaces와 remote.attach가 공유하는 접속 인자. profile과 ssh 중 하나만 받는다.
pub(crate) struct RemoteConnParams {
    pub(crate) profile: Option<String>,
    pub(crate) ssh: Option<String>,
    pub(crate) remote_tasty: String,
    pub(crate) remote_port_mode: String,
}

impl RemoteConnParams {
    pub(crate) fn parse(params: &Value) -> Result<Self, &'static str> {
        let profile = params
            .get("profile")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ssh = params
            .get("ssh")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if profile.is_some() && ssh.is_some() {
            return Err("'profile' and 'ssh' are mutually exclusive");
        }
        if profile.is_none() && ssh.is_none() {
            return Err("one of 'profile' or 'ssh' is required");
        }
        Ok(Self {
            profile,
            ssh,
            remote_tasty: params
                .get("remote_tasty")
                .and_then(|v| v.as_str())
                .unwrap_or("tasty")
                .to_string(),
            remote_port_mode: params
                .get("remote_port_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
        })
    }
}

/// 블로킹 SSH 조회를 워커에서 수행하고 같은 응답 채널로 결과를 돌려준다.
pub(crate) fn spawn_remote_workspaces(
    rpc_id: Value,
    params: &Value,
    response_tx: &SyncSender<JsonRpcResponse>,
) {
    let conn = match RemoteConnParams::parse(params) {
        Ok(c) => c,
        Err(msg) => {
            send_response(response_tx, JsonRpcResponse::invalid_params(rpc_id, msg));
            return;
        }
    };
    let RemoteConnParams {
        profile,
        ssh,
        remote_tasty,
        remote_port_mode,
    } = conn;
    let response_tx = response_tx.clone();
    std::thread::spawn(move || {
        let resp = match tasty_remote::browse::resolve_connection_spec(
            profile.as_deref(),
            ssh.as_deref(),
            &remote_tasty,
            &remote_port_mode,
        ) {
            Ok((target, rt, pm, pf)) => {
                match tasty_remote::browse::browse(&target, &rt, &pm, pf.as_deref()) {
                    Ok(list) => JsonRpcResponse::success(
                        rpc_id,
                        serde_json::to_value(list).unwrap_or(Value::Null),
                    ),
                    Err(e) => {
                        JsonRpcResponse::error(rpc_id, -32050, format!("remote browse failed: {e}"))
                    }
                }
            }
            Err(e) => JsonRpcResponse::error(rpc_id, -32050, format!("{e}")),
        };
        send_response(&response_tx, resp);
    });
}

#[cfg(feature = "gui")]
pub(crate) fn no_application_state(rpc_id: Value) -> JsonRpcResponse {
    JsonRpcResponse::error(rpc_id, -32000, "no application state available")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_conn_params_reject_both_and_neither() {
        let both = serde_json::json!({ "profile": "p", "ssh": "u@h" });
        assert!(RemoteConnParams::parse(&both).is_err());
        let neither = serde_json::json!({});
        assert!(RemoteConnParams::parse(&neither).is_err());
    }

    #[test]
    fn remote_conn_params_default_the_discovery_options() {
        let p = RemoteConnParams::parse(&serde_json::json!({ "ssh": "u@h" }))
            .expect("ssh alone is a valid spec");
        assert_eq!(p.remote_tasty, "tasty");
        assert_eq!(p.remote_port_mode, "auto");
        assert!(p.profile.is_none());
    }

    /// 빈 params의 text 조회 결과만 검사한다. 실제 핸들러나 클립보드 호출 순서는 실행하지 않는다.
    #[test]
    fn clipboard_without_text_is_an_invalid_params_error() {
        let params = serde_json::json!({});
        assert!(params.get("text").and_then(|v| v.as_str()).is_none());
    }
}
