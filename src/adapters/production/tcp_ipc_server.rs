//! Production adapter — `TcpIpcServer`. 옛 `IpcServer` (TCP listener + mpsc
//! channel + accept thread) 의 production 구현.
//!
//! D.3.D.2.a 의 본 commit 은 *trait impl + type alias* 만 — 구조체 본문 이전과
//! port_file static helper 분리는 D.3.D.2.b 에서 진행.

use std::sync::mpsc;

use crate::ipc::server::IpcCommand;
pub use crate::ipc::server::IpcServer as TcpIpcServer;
use crate::ports::ipc_server::IpcServerPort;

impl IpcServerPort for TcpIpcServer {
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        TcpIpcServer::try_recv(self)
    }

    fn port(&self) -> u16 {
        TcpIpcServer::port(self)
    }
}
