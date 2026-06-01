//! IPC wire 타입 + 응답 헬퍼 — `IpcCommand` / `IpcWaker` / `send_response`.
//!
//! 서버 인스턴스 본문은 `crate::adapters::production::tcp_ipc_server::TcpIpcServer`
//! (D.3.D.2.b) 로 이전. 본 모듈은 wire 형식과 강결합된 타입 정의만 보유 —
//! verify 자율 결정으로 ports/ 가 아닌 wire 모듈 옆에 둔다.

use std::sync::{Arc, mpsc};

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
}

/// IPC 응답 송신용 헬퍼. 클라이언트가 응답 전에 연결을 끊었거나 receiver가 drop된
/// 경우(`SendError`)에는 trace로만 흔적을 남긴다 — 정상적인 take-and-go 케이스라
/// warn 레벨로 올릴 만한 사건은 아니다.
pub fn send_response(tx: &mpsc::SyncSender<JsonRpcResponse>, response: JsonRpcResponse) {
    if let Err(e) = tx.send(response) {
        tracing::trace!("IPC response dropped (client disconnected): {e}");
    }
}

/// Callback to wake the main event loop when an IPC command arrives.
pub type IpcWaker = Arc<dyn Fn() + Send + Sync>;
