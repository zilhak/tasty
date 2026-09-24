//! TCP 없이 명령을 넣고 받는 시험용 IPC 서버. port는 0이다.

use std::sync::{Mutex, mpsc};

use crate::ipc::server::IpcCommand;
use crate::ports::ipc_server::IpcServerPort;

/// 실제 서버와 같은 try_recv 동작을 쓰도록 mpsc 채널로 구현한다.
pub struct MockIpcServer {
    rx: Mutex<mpsc::Receiver<IpcCommand>>,
    tx: mpsc::Sender<IpcCommand>,
    port: u16,
}

impl MockIpcServer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rx: Mutex::new(rx),
            tx,
            port: 0,
        }
    }

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

    fn command_sender(&self) -> mpsc::Sender<IpcCommand> {
        self.tx.clone()
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
        let cmd = IpcCommand::new(
            JsonRpcRequest {
                response_timeout_ms: None,
                idempotency_key: None,
                jsonrpc: "2.0".to_string(),
                method: "test.method".to_string(),
                id: Some(Value::from(1u64)),
                params: Value::Null,
                session_token: None,
            },
            resp_tx,
        );
        tx.send(cmd).expect("send to mock");
        let got = <MockIpcServer as IpcServerPort>::try_recv(&m).expect("recv");
        assert_eq!(got.request.method, "test.method");
    }
}
