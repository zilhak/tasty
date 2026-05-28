//! step 1: session_token → CallerContext 결정 + Agent caller 의 `ensure_allowed`.
//!
//! 토큰이 없으면 `Local`. 있는데 invalid/expired/revoked 면 `permission_denied` 로
//! 즉시 거부 (Local fallback 금지 — 위조 방어). Agent 가 통과 못한 메서드는
//! audit 기록 + (가능하면) capability elevation 발행.

use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::caller::resolve_caller_from_envelope;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    /// 반환: Some(caller) = 통과, None = 응답이 이미 전송된 거부.
    pub(crate) fn ipc_resolve_caller(
        &mut self,
        cmd: &IpcCommand,
    ) -> Option<host_ipc::caller::CallerContext> {
        let caller = match resolve_caller_from_envelope(&cmd.request) {
            Ok(c) => c,
            Err(resp) => {
                send_response(&cmd.response_tx, resp);
                return None;
            }
        };
        if matches!(caller, host_ipc::caller::CallerContext::Local) {
            return Some(caller);
        }
        let Err(e) = caller.ensure_allowed(&cmd.request.method) else {
            return Some(caller);
        };
        tracing::warn!("ipc agent caller denied: {e}");
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        // Phase 6.5a audit: app-level dispatcher 의 deny 도 기록.
        if let Some(main) = self.windows.values_mut().find_map(|w| w.as_main_mut()) {
            let ws = main
                .engine_state
                .workspaces
                .get(main.state.active_workspace)
                .map(|w| w.id);
            let seq = main.engine_state.telemetry_seq.next();
            host_ipc::audit::record(
                &caller,
                &cmd.request.method,
                host_ipc::audit::AuditDecision::Deny,
                Some(&format!("{e}")),
                ws,
                seq,
            );
        }
        // Phase 6.4a — Agent caller 의 MissingPermission 은 elevation 발행.
        // NotPluginCallable/UnknownMethod 는 elevation 으로 회복되지 않으므로 단순 deny.
        let mut data = serde_json::json!(null);
        if let (
            host_ipc::caller::CallerError::MissingPermission { permission, .. },
            host_ipc::caller::CallerContext::Agent { agent_id, .. },
        ) = (&e, &caller)
        {
            let agent_id = agent_id.clone();
            let perm_token = permission.as_token();
            let method = cmd.request.method.clone();
            let core = &mut self.core;
            let main = self.windows.values_mut().find_map(|w| w.as_main_mut());
            if let Some(m) = main {
                if let Some(rec) = host_ipc::handler::approval::publish_capability_elevation(
                    core,
                    &mut m.state,
                    &mut m.engine_state,
                    &agent_id,
                    &method,
                    &perm_token,
                    None,
                ) {
                    data = serde_json::json!({
                        "kind": "capability_elevation",
                        "approval_id": rec.request.id,
                        "permission": perm_token,
                        "method": method,
                    });
                }
            }
        }
        let mut response = host_ipc::protocol::JsonRpcResponse::error(
            rpc_id,
            -32001,
            &format!("permission_denied: {e}"),
        );
        if !data.is_null()
            && let Some(err) = response.error.as_mut()
        {
            err.data = Some(data);
        }
        send_response(&cmd.response_tx, response);
        None
    }
}
