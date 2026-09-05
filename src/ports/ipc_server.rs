//! Outbound port — IPC server 추상화. Hub 가 보유.
//!
//! production: `TcpIpcServer`(`src/adapters/production/tcp_ipc_server.rs`).
//! 인스턴스 생성 시그니처는 *production 마다 다를 수 있어* trait 표면에
//! 포함하지 않는다 (Hub 가 production adapter 의 concrete `start_*` 호출).
//!
//! Trait 표면은 *런타임 polling 만* — `try_recv()` + `port()`. shutdown 은 Drop.
//!
//! `IpcCommand` / `IpcWaker` 타입은 옛 위치 (`crate::ipc::server`) 유지 — wire 형식
//! (JSON-RPC) 과 강결합이라 trait 옆이 아니라 wire 모듈에 두는 게 자연스럽다.

use std::sync::mpsc;

use crate::ipc::server::IpcCommand;

/// IPC server adapter — Hub 가 `Option<Box<dyn IpcServerPort>>` 로 보유.
///
/// `Send` 만 요구 — Hub 는 단일 owner (App) 에서만 polling 한다. `mpsc::Receiver`
/// 가 `!Sync` 라 `Sync` bound 는 부착할 수 없다.
pub trait IpcServerPort: Send {
    /// 큐에서 IPC command 1 개 비차단으로 꺼낸다.
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError>;
    /// 서버가 listen 중인 포트 (debug/log 표시용).
    fn port(&self) -> u16;
    /// 외부 (runner thread 등 off-main) 에서 큐에 `IpcCommand` 를 inject 할 때
    /// 사용하는 sender 의 사본. host→plugin sync dispatch 전용.
    fn command_sender(&self) -> mpsc::Sender<IpcCommand>;
}
