//! IPC wire 타입 + 응답 헬퍼 — `IpcCommand` / `IpcWaker` / `send_response`.
//!
//! 서버 인스턴스 본문은 `crate::adapters::production::tcp_ipc_server::TcpIpcServer`
//! (D.3.D.2.b) 로 이전. 본 모듈은 wire 형식과 강결합된 타입 정의만 보유 —
//! verify 자율 결정으로 ports/ 가 아닌 wire 모듈 옆에 둔다.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
    /// 이 명령이 큐에 들어간 순간(monotonic).
    ///
    /// 큐에서 기다린 시간과 handler 안에서 보낸 시간은 **다른 값**인데, 이 표시가 없으면
    /// 둘을 가를 수 없다 — 응답이 느린 것만 보이고 그것이 적체인지 handler 비용인지
    /// 고를 수 없다.
    ///
    /// `Clock` port 가 아니라 `Instant::now()` 인 이유: 명령을 만드는 두 자리(소켓 accept
    /// 스레드와 plugin host-call 주입부)는 둘 다 `Core` 를 들고 있지 않다. 여기서 재는
    /// 것은 도메인 시각이 아니라 큐 체류 시간이라 monotonic 원천이면 충분하다.
    pub enqueued_at: Instant,
}

impl IpcCommand {
    /// 지금을 큐 진입 시각으로 찍어 명령을 만든다.
    ///
    /// 구조체 리터럴 대신 이 생성자를 쓰는 이유는 새 주입 경로가 시각을 **빠뜨릴 수
    /// 없게** 하려는 것이다 — 빠뜨리면 그 경로의 대기 시간만 조용히 0 이 된다.
    pub fn new(request: JsonRpcRequest, response_tx: mpsc::SyncSender<JsonRpcResponse>) -> Self {
        Self {
            request,
            response_tx,
            enqueued_at: Instant::now(),
        }
    }

    /// 큐에 들어간 뒤 지금까지 기다린 시간.
    pub fn queue_wait(&self) -> Duration {
        self.enqueued_at.elapsed()
    }
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
