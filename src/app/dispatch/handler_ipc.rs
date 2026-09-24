//! 파일 핸들러가 요청한 IPC를 플러그인으로 전달한다.

use crate::app::App;

impl App {
    /// 응답은 기다리지 않는다. 처리 결과는 플러그인의 로그·이벤트로 확인한다.
    pub(crate) fn dispatch_pending_handler_ipc(&mut self) {
        let mut drained: Vec<(String, crate::file::format::FileTarget)> = Vec::new();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                drained.append(&mut main.state.pending_handler_ipc);
            }
        }
        for (s, _engine) in &mut self.parked_states {
            drained.append(&mut s.pending_handler_ipc);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            for (method, target) in drained {
                tracing::warn!(
                    method = %method,
                    target = %target.display(),
                    "file handler IPC action dropped: plugin manager not running",
                );
            }
            return;
        };
        for (method, target) in drained {
            let params = serde_json::json!({
                "path": target.as_path().to_string_lossy(),
            });
            let (tx, _rx) = std::sync::mpsc::sync_channel(1);
            // 큐가 원 요청 번호를 보존하지 않아 IPC 처리 이력의 원 요청과 연결하지 못한다.
            mgr.forward_namespace_call(&method, params, None, serde_json::Value::Null, tx, None);
        }
    }
}
