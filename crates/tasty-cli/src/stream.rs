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
        Self::open_with(stream, proto, None, None)
    }

    /// Like [`open`](Self::open) but requests attach to `target` (a surface_id):
    /// the server acquires the exclusive lock, pushes the initial screen
    /// snapshot, then streams output (attach/detach step 4). The attach result
    /// (success/`attach_error`) arrives as a *separate* `Control` frame after the
    /// handshake ack — callers must read it (see `commands::attach`).
    pub fn open_attach(stream: TcpStream, proto: u32, target: u32) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, Some(target), None)
    }

    /// Like [`open_attach`](Self::open_attach) but attaches a whole workspace
    /// (attach/detach step 6): the server mirrors every terminal in the workspace
    /// and the connection's `Data` frames are surface-prefixed (`decode_mux`).
    pub fn open_attach_workspace(
        stream: TcpStream,
        proto: u32,
        workspace: u32,
    ) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, Some(workspace))
    }

    fn open_with(
        stream: TcpStream,
        proto: u32,
        target: Option<u32>,
        target_workspace: Option<u32>,
    ) -> Result<(Self, u32)> {
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
