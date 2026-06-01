//! MockIpcServer — in-memory `IpcServerPort`.
//!
//! 테스트에서 IPC server 의존을 *TCP 없이* 검증할 때 사용. `push` 로 command 를
//! 큐에 직접 넣고, Hub 소비 측은 `try_recv` 로 꺼낸다. port 는 0 고정.

use std::sync::{Mutex, mpsc};

use crate::ipc::server::IpcCommand;
use crate::ports::ipc_server::IpcServerPort;

/// In-memory IPC server. command queue 는 `Mutex<VecDeque<IpcCommand>>` 가
/// 아닌 `mpsc::channel` 로 — 실제 production 과 동일한 try_recv 의미를 유지.
pub struct MockIpcServer {
    rx: Mutex<mpsc::Receiver<IpcCommand>>,
    tx: mpsc::Sender<IpcCommand>,
    port: u16,
}

impl MockIpcServer {
    /// `port=0` 으로 새 mock 생성. `tx` 는 caller 가 외부에서 push 할 수 있도록
    /// `tx_clone()` 로 받아간다.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rx: Mutex::new(rx),
            tx,
            port: 0,
        }
    }

    /// 테스트가 command 를 큐에 넣을 수 있는 sender 사본을 받는다.
    pub fn tx_clone(&self) -> mpsc::Sender<IpcCommand> {
        self.tx.clone()
    }
}

impl Default for MockIpcServer {
    fn default() -> Self {
        Self::new()
    }
}

impl IpcServerPort for MockIpcServer {
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        let rx = self.rx.lock().expect("MockIpcServer rx poisoned");
        rx.try_recv()
    }

    fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use serde_json::Value;

    use super::*;
    use crate::ipc::protocol::JsonRpcRequest;
    use crate::ports::ipc_server::IpcServerPort;

    #[test]
    fn empty_returns_disconnected_or_empty() {
        let m = MockIpcServer::new();
        // 새 mock 은 비어 있다 — Empty 반환.
        assert!(matches!(
            <MockIpcServer as IpcServerPort>::try_recv(&m),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(<MockIpcServer as IpcServerPort>::port(&m), 0);
    }

    #[test]
    fn push_then_try_recv_returns_command() {
        let m = MockIpcServer::new();
        let tx = m.tx_clone();
        let (resp_tx, _resp_rx) = mpsc::sync_channel(1);
        let cmd = IpcCommand {
            request: JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "test.method".to_string(),
                id: Some(Value::from(1u64)),
                params: Value::Null,
                session_token: None,
            },
            response_tx: resp_tx,
        };
        tx.send(cmd).expect("send to mock");
        let got = <MockIpcServer as IpcServerPort>::try_recv(&m).expect("recv");
        assert_eq!(got.request.method, "test.method");
    }
}
