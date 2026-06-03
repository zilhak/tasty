//! Host→plugin sync IPC dispatch — runner thread 등 off-main thread 가 plugin
//! IPC 메서드를 동기 호출할 때 사용.
//!
//! App 의 IPC command 큐에 `IpcCommand` 를 직접 주입하고 `sync_channel(1)` 의
//! recv_timeout 으로 응답을 기다린다. `IpcWaker` 를 호출해 main loop 가 즉시
//! 깨어나도록 한다. main loop 가 다음 tick 에서 `routing.rs` 의 plugin namespace
//! 포워딩 경로로 plugin worker 에 디스패치 → plugin 응답이 같은 sync_channel 로
//! 회신되면 본 호출자가 받는다.

use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::{IpcCommand, IpcWaker};

/// Host→plugin 동기 IPC 디스패처. `Clone` — Arc 기반이라 thread 간 자유 공유.
#[derive(Clone)]
pub struct HostIpcInjector {
    sender: mpsc::Sender<IpcCommand>,
    waker: IpcWaker,
}

impl HostIpcInjector {
    pub fn new(sender: mpsc::Sender<IpcCommand>, waker: IpcWaker) -> Self {
        Self { sender, waker }
    }

    /// JSON-RPC 메서드를 동기 호출. 응답은 `Result::Ok(result)` 또는 RPC 에러를
    /// `Err(message)` 로 반환. timeout 은 응답 대기 시간 (App tick + plugin 처리
    /// 시간 모두 포함).
    pub fn dispatch(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let (resp_tx, resp_rx) = mpsc::sync_channel::<JsonRpcResponse>(1);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            id: Some(Value::from(1u64)),
            params,
            session_token: None,
        };
        let cmd = IpcCommand {
            request: req,
            response_tx: resp_tx,
        };
        self.sender
            .send(cmd)
            .map_err(|e| format!("inject IpcCommand: {e}"))?;
        (self.waker)();
        match resp_rx.recv_timeout(timeout) {
            Ok(resp) => {
                if let Some(err) = resp.error {
                    Err(format!("rpc error {}: {}", err.code, err.message))
                } else {
                    Ok(resp.result.unwrap_or(Value::Null))
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(format!("host_dispatch timeout after {:?}", timeout))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("host_dispatch response channel disconnected".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    fn noop_waker() -> IpcWaker {
        Arc::new(|| {})
    }

    #[test]
    fn dispatch_sends_command_and_returns_response() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());

        // worker thread 가 cmd 를 받아 echo 응답 전송.
        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
            assert_eq!(cmd.request.method, "echo.method");
            let resp = JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(Value::Null),
                serde_json::json!({"ok": true}),
            );
            cmd.response_tx.send(resp).expect("send resp");
        });

        let v = injector
            .dispatch("echo.method", serde_json::json!({}), Duration::from_secs(2))
            .expect("dispatch ok");
        assert_eq!(v, serde_json::json!({"ok": true}));
        h.join().unwrap();
    }

    #[test]
    fn dispatch_returns_err_on_rpc_error() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());

        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
            let resp = JsonRpcResponse::error(
                cmd.request.id.clone().unwrap_or(Value::Null),
                -32601,
                "Method not found",
            );
            cmd.response_tx.send(resp).expect("send resp");
        });

        let err = injector
            .dispatch("missing", Value::Null, Duration::from_secs(2))
            .expect_err("should be err");
        assert!(err.contains("-32601"));
        h.join().unwrap();
    }

    #[test]
    fn dispatch_times_out_when_no_response() {
        let (tx, _rx) = mpsc::channel::<IpcCommand>();
        // rx 는 drop 되지 않게 보유 — Disconnected 가 아니라 Timeout 을 받아야 함.
        let injector = HostIpcInjector::new(tx, noop_waker());
        let err = injector
            .dispatch("noop", Value::Null, Duration::from_millis(50))
            .expect_err("should timeout");
        assert!(err.contains("timeout"));
    }
}
