//! `workspace.list` / `surface.list` / `pane.list` 가 단일 engine 만 보지 않고
//! 모든 main + parked engine 을 순회해 결과를 합치도록 호스트 레벨에서 special-case
//! 처리한다. CLAUDE.md "list 명령은 전체 워크스페이스를 순회" 원칙.

use serde_json::json;

use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::handler::{pane, surface, workspace};
use crate::ipc::protocol::JsonRpcResponse;

impl App {
    /// list 류 메서드면 모든 engine 결과를 합쳐 반환. 그 외는 None 반환해
    /// caller 가 일반 routing 계속.
    pub(crate) fn dispatch_list_global(
        &self,
        request: &host_ipc::protocol::JsonRpcRequest,
    ) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        match request.method.as_str() {
            "workspace.list" => {
                Some(self.collect_list(id, |s, e, i| workspace::handle_workspace_list(s, e, i)))
            }
            "surface.list" => {
                Some(self.collect_list(id, |s, e, i| surface::handle_surface_list(s, e, i)))
            }
            "pane.list" => Some(self.collect_list(id, |s, e, i| pane::handle_pane_list(s, e, i))),
            _ => None,
        }
    }

    fn collect_list<F>(&self, id: serde_json::Value, f: F) -> JsonRpcResponse
    where
        F: Fn(
            &crate::state::AppState,
            &crate::engine_state::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let mut combined: Vec<serde_json::Value> = Vec::new();
        for w in self.windows.values() {
            if let Some(m) = w.as_main() {
                let resp = f(&m.state, &m.engine_state, id.clone());
                if let Some(arr) = resp.result.as_ref().and_then(|v| v.as_array()) {
                    combined.extend(arr.iter().cloned());
                }
            }
        }
        for (s, e) in &self.parked_states {
            let resp = f(s, e, id.clone());
            if let Some(arr) = resp.result.as_ref().and_then(|v| v.as_array()) {
                combined.extend(arr.iter().cloned());
            }
        }
        JsonRpcResponse::success(id, json!(combined))
    }
}
