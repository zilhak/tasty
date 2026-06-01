//! file handler 가 enqueue 한 IPC action 을 plugin namespace 메서드로 forward.

use crate::app::App;

impl App {
    /// file handler IPC action 큐 drain. user TOML 등에서 `type="ipc"` 인 핸들러가
    /// 매칭되면 `(method, target)` 이 enqueue 되어 여기서 plugin namespace 메서드로
    /// forward 된다. 응답은 무시 (fire-and-forget) — 핸들러 실행 결과는 plugin 자체
    /// 로그/이벤트로 관찰.
    pub(crate) fn dispatch_pending_handler_ipc(&mut self) {
        let mut drained: Vec<(String, crate::file::format::FileTarget)> = Vec::new();
        for w in self.view.windows.values_mut() {
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
            mgr.forward_namespace_call(&method, params, None, serde_json::Value::Null, tx);
        }
    }
}
