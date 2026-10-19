//! 파일 핸들러가 요청한 IPC를 플러그인으로 전달한다. 결과를 기다리는 요청은 플러그인 응답을
//! 받아 그 run([`crate::request_runs`])을 끝낸다.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};

use tasty_ipc::protocol::JsonRpcResponse;

use crate::app::App;
use crate::request_runs::RunState;
#[cfg(test)]
use serde_json::Value;

/// 응답을 기다리는 (run, 메서드, 응답 채널). 메인 루프만 쓰지만 App 필드를 늘리지 않으려 정적 목록에 둔다.
static WAITING: Mutex<Vec<(u64, String, Receiver<JsonRpcResponse>)>> = Mutex::new(Vec::new());

impl App {
    pub(crate) fn dispatch_pending_handler_ipc(&mut self) {
        finish_answered_handler_ipc();
        let mut drained: Vec<(String, crate::file::format::FileTarget, Option<u64>)> = Vec::new();
        for (s, _engine) in self.engines_mut().sessions() {
            drained.append(&mut s.pending_handler_ipc);
        }
        if drained.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            for (method, target, run) in drained {
                tracing::warn!(
                    method = %method,
                    target = %target.display(),
                    "file handler IPC action dropped: plugin manager not running",
                );
                crate::file::dispatch::fail_run(
                    run,
                    format!("{method} was not sent: plugins are not running"),
                );
            }
            return;
        };
        for (method, target, run) in drained {
            let params = serde_json::json!({
                "path": target.as_path().to_string_lossy(),
            });
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            mgr.forward_namespace_call(&method, params, None, serde_json::Value::Null, tx, None);
            if let Some(run) = run {
                lock_waiting().push((run, method, rx));
            }
        }
        finish_answered_handler_ipc();
    }
}

fn lock_waiting() -> std::sync::MutexGuard<'static, Vec<(u64, String, Receiver<JsonRpcResponse>)>> {
    WAITING.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("file handler IPC wait list was poisoned; keeping its entries");
        poisoned.into_inner()
    })
}

/// 응답이 온 요청과 응답 없이 채널이 닫힌 요청의 run을 끝낸다.
fn finish_answered_handler_ipc() {
    lock_waiting().retain(|(run, method, rx)| match rx.try_recv() {
        Ok(response) => {
            finish_with_response(*run, method, &response);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => {
            crate::file::dispatch::fail_run(
                Some(*run),
                format!("{method} ended without an answer from the plugin"),
            );
            false
        }
    });
}

fn finish_with_response(run: u64, method: &str, response: &JsonRpcResponse) {
    match &response.error {
        Some(error) => crate::file::dispatch::finish_run(
            Some(run),
            RunState::Failed,
            format!("{method} failed: {}", error.message),
            serde_json::json!({ "code": error.code }),
        ),
        None => crate::file::dispatch::finish_run(
            Some(run),
            RunState::Succeeded,
            format!("{method} answered"),
            serde_json::json!({ "response": response.result }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_runs::{begin, lookup};

    fn error(code: i32, message: &str) -> JsonRpcResponse {
        JsonRpcResponse::error(serde_json::Value::Null, code, message.to_string())
    }

    #[test]
    fn the_plugin_answer_or_its_absence_ends_the_run() {
        let answered = begin("file_handler.dispatch", None, Value::Null);
        let refused = begin("file_handler.dispatch", None, Value::Null);
        let vanished = begin("file_handler.dispatch", None, Value::Null);
        let waiting = begin("file_handler.dispatch", None, Value::Null);
        let (ok_tx, ok_rx) = std::sync::mpsc::sync_channel(1);
        let (err_tx, err_rx) = std::sync::mpsc::sync_channel(1);
        let (gone_tx, gone_rx) = std::sync::mpsc::sync_channel::<JsonRpcResponse>(1);
        let (_wait_tx, wait_rx) = std::sync::mpsc::sync_channel::<JsonRpcResponse>(1);
        lock_waiting().extend([
            (answered, "example.open".to_string(), ok_rx),
            (refused, "example.open".to_string(), err_rx),
            (vanished, "example.open".to_string(), gone_rx),
            (waiting, "example.open".to_string(), wait_rx),
        ]);
        ok_tx
            .send(JsonRpcResponse::success(
                serde_json::Value::Null,
                serde_json::json!({"opened": true}),
            ))
            .unwrap();
        err_tx.send(error(-32000, "cannot open")).unwrap();
        drop(gone_tx);
        finish_answered_handler_ipc();

        let ok = lookup(answered, None).unwrap();
        assert_eq!(ok["state"], "succeeded");
        assert_eq!(ok["response"], serde_json::json!({"opened": true}));
        let err = lookup(refused, None).unwrap();
        assert_eq!(err["state"], "failed");
        assert_eq!(err["result"], "example.open failed: cannot open");
        assert_eq!(err["code"], -32000);
        assert_eq!(lookup(vanished, None).unwrap()["state"], "failed");
        assert_eq!(lookup(waiting, None).unwrap()["state"], "running");
        lock_waiting().retain(|(run, ..)| *run != waiting);
    }
}
