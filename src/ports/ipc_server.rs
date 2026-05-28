//! IpcServerPort — Hub 의 외부 통신 trait.
//!
//! Production 은 `std::net::TcpListener`. Test 는 in-process channel.

use std::path::Path;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// IPC 서버. 사용자 / plugin / 외부 client 가 TCP 또는 channel 로 명령 전송.
#[allow(dead_code)]
pub trait IpcServerPort: Send + Sync {
    /// 서버 start. 반환: 실제 listening port (0 이면 random). `port_file` 에 기록.
    fn start(&mut self, port_file: Option<&Path>) -> anyhow::Result<u16>;

    /// non-blocking 으로 큐에서 명령 한 개 꺼냄.
    fn try_recv(&self) -> anyhow::Result<Option<IpcCommand>>;

    /// 응답 송신.
    fn send_response(&self, response: JsonRpcResponse) -> anyhow::Result<()>;

    fn shutdown(&mut self);
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub respond_to: ResponseChannel,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct ResponseChannel {
    /// 구현에 따라 conn_id 또는 channel handle.
    pub opaque: u64,
}
