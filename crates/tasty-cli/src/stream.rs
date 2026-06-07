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
        let writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        let mut writer = writer;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: stream::STREAM_OPEN_METHOD.to_string(),
            params: serde_json::json!({ "proto": proto }),
            id: Some(serde_json::json!(1)),
            session_token: None,
        };
        writeln!(writer, "{}", serde_json::to_string(&req)?)?;
        writer.flush()?;

        let ack_frame = stream::read_frame(&mut reader)?;
        if ack_frame.tag != StreamTag::Control {
            bail!(
                "expected Control ack frame, got {:?}",
                ack_frame.tag
            );
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
