//! `tool.clipboard.clear` / `tool.clipboard.remove` 를 모든 main + parked engine
//! 에 broadcast 한다. multi-window 에서 한 윈도우의 clear/remove 가 다른 윈도우의
//! history 에도 반영되어야 사용자 일관성. record path 는 이미
//! `app/clipboard_record.rs::record_clipboard_data` 가 broadcast 처리 중.

use serde_json::{Value, json};

use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::protocol::JsonRpcResponse;

impl App {
    /// clear/remove 면 모든 engine 에 적용 후 응답 반환, 그 외는 None.
    pub(crate) fn dispatch_clipboard_global(
        &mut self,
        request: &host_ipc::protocol::JsonRpcRequest,
    ) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(Value::Null);
        match request.method.as_str() {
            "tool.clipboard.clear" => {
                for w in self.view.windows.values_mut() {
                    if let Some(m) = w.as_main_mut() {
                        m.engine_state.clipboard_history.clear();
                    }
                }
                for (_, e) in self.parked_states.iter_mut() {
                    e.clipboard_history.clear();
                }
                Some(JsonRpcResponse::success(id, json!({ "ok": true })))
            }
            "tool.clipboard.remove" => {
                let idx = match request.params.get("index").and_then(|v| v.as_u64()) {
                    Some(n) => n as usize,
                    None => {
                        return Some(JsonRpcResponse::invalid_params(id, "Missing 'index'"));
                    }
                };
                // 모든 engine 의 같은 idx entry 제거. record_clipboard_data 가 모든
                // engine 에 동일 순서로 push 하므로 idx 도 동일 entry 가리킴.
                let mut removed_any = false;
                for w in self.view.windows.values_mut() {
                    if let Some(m) = w.as_main_mut() {
                        if m.engine_state.clipboard_history.remove_at(idx).is_some() {
                            removed_any = true;
                        }
                    }
                }
                for (_, e) in self.parked_states.iter_mut() {
                    if e.clipboard_history.remove_at(idx).is_some() {
                        removed_any = true;
                    }
                }
                if removed_any {
                    Some(JsonRpcResponse::success(
                        id,
                        json!({ "ok": true, "index": idx }),
                    ))
                } else {
                    Some(JsonRpcResponse::invalid_params(
                        id,
                        format!("Index {idx} out of range"),
                    ))
                }
            }
            _ => None,
        }
    }
}
