//! Client-side streaming-channel transport (attach/detach step 1).
//!
//! Upgrades a freshly connected TCP socket into the framed streaming channel by
//! sending the `stream.open` handshake line, then reading the server's `Control`
//! ack frame. After that the socket carries length-prefixed binary frames in
//! both directions (see `tasty_ipc::stream`).
//!
//! Transport only — attach semantics arrive in later steps.

use std::io::{BufReader, Write};
use std::net::TcpStream;

use anyhow::{Result, bail};

use tasty_ipc::protocol::JsonRpcRequest;
use tasty_ipc::stream::{self, StreamAck, StreamFrame, StreamTag};

/// A streaming-channel connection to a running tasty instance.
pub struct StreamConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl StreamConnection {
    /// Upgrade a connected TCP socket into a streaming channel.
    ///
    /// Sends the `stream.open` handshake line, then blocks for the server's
    /// `Control` ack frame. Returns the connection and the server-assigned
    /// client id.
    pub fn open(stream: TcpStream, proto: u32) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, None, None)
    }

    /// Like [`open`](Self::open) but requests attach to `target` (a surface_id):
    /// the server acquires the exclusive lock, pushes the initial screen
    /// snapshot, then streams output (attach/detach step 4). The attach result
    /// (success/`attach_error`) arrives as a *separate* `Control` frame after the
    /// handshake ack — callers must read it (see `commands::attach`).
    pub fn open_attach(stream: TcpStream, proto: u32, target: u32) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, Some(target), None, None)
    }

    /// Like [`open_attach`](Self::open_attach) but attaches a whole workspace
    /// (attach/detach step 6): the server mirrors every terminal in the workspace
    /// and the connection's `Data` frames are surface-prefixed (`decode_mux`).
    pub fn open_attach_workspace(
        stream: TcpStream,
        proto: u32,
        workspace: u32,
    ) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, Some(workspace), None)
    }

    /// bulk 파일 전송 전용 연결(ADR-0053)로 업그레이드한다. `open_attach_workspace`
    /// 와 달리 workspace 를 mirror 하지 않고(= holder 가 되지 않고), 이 연결의 `Data`
    /// 프레임을 서버가 파일 청크(`encode_bulk_chunk`)로 분류하도록 bulk 로 태깅한다.
    /// `workspace` 는 저장·인가의 결속 대상(서버는 그 ws 에 활성 holder 가 있을 때만
    /// 수락). 같은 `ssh -L` 터널의 `127.0.0.1:<local_port>` 에 두 번째로 열어 대화형
    /// attach 스트림과 소켓을 분리한다(HOL 방지). ※ 06-β(클라 송신)가 이 진입점을
    /// 호출한다 — 06-α 는 시그니처만 정의.
    pub fn open_bulk(stream: TcpStream, proto: u32, workspace: u32) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, None, Some(workspace))
    }

    fn open_with(
        stream: TcpStream,
        proto: u32,
        target: Option<u32>,
        target_workspace: Option<u32>,
        bulk_workspace: Option<u32>,
    ) -> Result<(Self, u32)> {
        // 조용한 네트워크 단절 감지용 read timeout. writer/reader 는 이 소켓의 clone이라
        // 옵션이 공유되므로 여기서 한 번만 걸면 이후 모든 `recv()`(핸드셰이크 ack 대기
        // 포함)에 적용된다.
        stream.set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT))?;
        let writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        let mut writer = writer;

        let mut params = serde_json::json!({ "proto": proto });
        if let Some(t) = target {
            params["target"] = serde_json::json!(t);
        }
        if let Some(w) = target_workspace {
            params["target_workspace"] = serde_json::json!(w);
        }
        if let Some(w) = bulk_workspace {
            params["bulk_workspace"] = serde_json::json!(w);
        }
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: stream::STREAM_OPEN_METHOD.to_string(),
            params,
            id: Some(serde_json::json!(1)),
            session_token: None,
        };
        writeln!(writer, "{}", serde_json::to_string(&req)?)?;
        writer.flush()?;

        let ack_frame = stream::read_frame(&mut reader)?;
        if ack_frame.tag != StreamTag::Control {
            bail!("expected Control ack frame, got {:?}", ack_frame.tag);
        }
        let ack: StreamAck = serde_json::from_slice(&ack_frame.payload)?;
        if !ack.ok {
            bail!(
                "stream.open rejected: {}",
                ack.error.unwrap_or_else(|| "unknown error".to_string())
            );
        }

        Ok((Self { writer, reader }, ack.client_id.unwrap_or(0)))
    }

    /// Clone the writer half of the socket so input can be sent from one thread
    /// while another blocks reading frames (mirror-dump / raw bridge).
    pub fn try_clone_writer(&self) -> Result<TcpStream> {
        Ok(self.writer.try_clone()?)
    }

    /// Write one frame to the server.
    pub fn send(&mut self, tag: StreamTag, payload: &[u8]) -> Result<()> {
        stream::write_frame(&mut self.writer, tag, payload)?;
        Ok(())
    }

    /// Block for the next frame from the server.
    pub fn recv(&mut self) -> Result<StreamFrame> {
        Ok(stream::read_frame(&mut self.reader)?)
    }

    /// Signal a graceful close to the server.
    pub fn detach(&mut self) -> Result<()> {
        stream::write_frame(&mut self.writer, StreamTag::Detach, &[])?;
        Ok(())
    }
}
