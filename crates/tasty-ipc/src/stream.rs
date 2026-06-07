//! Streaming channel codec for the attach/detach feature (step 1).
//!
//! A normal line-delimited JSON-RPC connection can be *upgraded* to a streaming
//! channel by sending `{"method":"stream.open",...}` as its first line. After the
//! upgrade the socket carries length-prefixed binary frames instead of JSON
//! lines, so the server can push bytes/events to the client continuously.
//!
//! Frame layout: `[tag: u8][len: u32 BE][payload: len bytes]`.
//!
//! Step 1 is *transport only* — attach semantics (lock / mirror / placeholder)
//! arrive in later steps. See `.claude-workspace/conductor/attach-detach/step1/plan.md`.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Method name whose presence as the first JSON-RPC line upgrades a connection
/// to the streaming channel.
pub const STREAM_OPEN_METHOD: &str = "stream.open";

/// Current streaming protocol version.
pub const STREAM_PROTO: u32 = 1;

/// Maximum accepted frame payload length (1 MiB). Frames larger than this are
/// rejected and the connection is closed — guards against malicious or runaway
/// length prefixes (wezterm issue #7527 OOM lesson).
pub const MAX_FRAME_LEN: u32 = 1 << 20;

/// Frame type tag (first byte of every frame).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum StreamTag {
    /// Raw bytes (future PTY output / echo payload).
    Data = 0,
    /// UTF-8 JSON control message (handshake ack; future resize/detach metadata).
    Control = 1,
    /// Keepalive ping. Reserved — unused in step 1.
    Ping = 2,
    /// Graceful close signal (empty payload), either direction.
    Detach = 3,
}

impl StreamTag {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Data),
            1 => Some(Self::Control),
            2 => Some(Self::Ping),
            3 => Some(Self::Detach),
            _ => None,
        }
    }
}

/// One streaming frame (tag + payload). Shared by the server push sink and the
/// client transport.
#[derive(Clone, Debug)]
pub struct StreamFrame {
    pub tag: StreamTag,
    pub payload: Vec<u8>,
}

impl StreamFrame {
    pub fn new(tag: StreamTag, payload: Vec<u8>) -> Self {
        Self { tag, payload }
    }
}

/// `params` of the `stream.open` handshake request.
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamOpenParams {
    #[serde(default)]
    pub proto: u32,
    /// attach 대상 surface_id. `Some` 이면 서버가 핸드셰이크 직후 그 surface 를
    /// 이 연결의 client 로 attach 한다(배타 점유 + 초기 스냅샷 + 출력 forward).
    /// `None` 이면 순수 스트림(단계 1 echo) — attach 의미 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
}

/// Control payload the server sends immediately after a successful upgrade.
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamAck {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<u32>,
    #[serde(default)]
    pub proto: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Write a single framed message (`[tag][len BE][payload]`), then flush.
pub fn write_frame<W: Write>(w: &mut W, tag: StreamTag, payload: &[u8]) -> io::Result<()> {
    let len: u32 = payload.len().try_into().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "frame payload exceeds u32")
    })?;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame payload exceeds MAX_FRAME_LEN",
        ));
    }
    w.write_all(&[tag as u8])?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Read a single framed message. Returns `Err` on EOF, an unknown tag, or a
/// length prefix exceeding [`MAX_FRAME_LEN`].
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<StreamFrame> {
    let mut hdr = [0u8; 5];
    r.read_exact(&mut hdr)?;
    let tag = StreamTag::from_u8(hdr[0])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown stream tag"))?;
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]);
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds MAX_FRAME_LEN",
        ));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    Ok(StreamFrame { tag, payload })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        for (tag, payload) in [
            (StreamTag::Data, b"hello world".to_vec()),
            (StreamTag::Control, br#"{"ok":true}"#.to_vec()),
            (StreamTag::Detach, Vec::new()),
            (StreamTag::Ping, vec![0u8; 1000]),
        ] {
            let mut buf = Vec::new();
            write_frame(&mut buf, tag, &payload).unwrap();
            let mut cur = Cursor::new(buf);
            let frame = read_frame(&mut cur).unwrap();
            assert_eq!(frame.tag, tag);
            assert_eq!(frame.payload, payload);
        }
    }

    #[test]
    fn tag_from_u8() {
        assert_eq!(StreamTag::from_u8(0), Some(StreamTag::Data));
        assert_eq!(StreamTag::from_u8(3), Some(StreamTag::Detach));
        assert_eq!(StreamTag::from_u8(9), None);
    }

    #[test]
    fn read_rejects_unknown_tag() {
        // tag=9 (invalid), len=0
        let bytes = vec![9u8, 0, 0, 0, 0];
        let mut cur = Cursor::new(bytes);
        assert!(read_frame(&mut cur).is_err());
    }

    #[test]
    fn read_rejects_oversize_len() {
        // tag=0, len=MAX_FRAME_LEN+1 — must reject before allocating.
        let len = MAX_FRAME_LEN + 1;
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(&len.to_be_bytes());
        let mut cur = Cursor::new(bytes);
        let err = read_frame(&mut cur).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_eof_is_err() {
        let mut cur = Cursor::new(Vec::new());
        assert!(read_frame(&mut cur).is_err());
    }

    #[test]
    fn write_rejects_oversize_payload() {
        let big = vec![0u8; (MAX_FRAME_LEN + 1) as usize];
        let mut buf = Vec::new();
        assert!(write_frame(&mut buf, StreamTag::Data, &big).is_err());
    }
}
