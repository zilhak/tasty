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
    /// attach 대상 workspace_id(단계 6). `Some` 이면 서버가 그 workspace 의 모든
    /// 터미널 surface 를 mirror 하고 비-터미널은 placeholder 로 숨긴다. 이 연결의
    /// 모든 `Data` 프레임은 **surface-prefixed**(`[u32 surface_id BE][bytes]`, D3)다.
    /// `target` 와 상호배타 — 둘 다 지정되면 서버가 거부한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_workspace: Option<u32>,
}

/// workspace attach(단계 6, D3) 의 `Data` 프레임 다중화 인코딩.
/// 페이로드 앞에 4바이트 BE surface_id 를 붙여 한 연결로 N 개 터미널의 바이트를
/// 구분해 실어 보낸다. surface 단위(단계 4) 연결은 이 prefix 를 쓰지 않는다(bare).
pub fn encode_mux(surface_id: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&surface_id.to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

/// [`encode_mux`] 의 역연산. 4바이트 미만이면 `None`(잘린 프레임).
pub fn decode_mux(buf: &[u8]) -> Option<(u32, &[u8])> {
    if buf.len() < 4 {
        return None;
    }
    let sid = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    Some((sid, &buf[4..]))
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

/// Server→client control events pushed over the attach stream *during* a live
/// session (after the handshake ack/descriptor). Tagged JSON on the `event`
/// field — **extensible**: new mid-session control events (e.g. structural ops:
/// tab/pane open/close) add a variant here without allocating a new
/// [`StreamTag`]. The one-shot handshake descriptors (`attached` /
/// `attached_workspace` / `attach_error`) and the `force_detached` signal remain
/// ad-hoc JSON read positionally by the client; this enum covers the streaming
/// control messages that arrive mid-session.
///
/// Clients deserialize each mid-session `Control` payload into this enum and act
/// on the variants they know; a payload that does not match any variant (an
/// older handshake shape or a newer event) fails to deserialize and is ignored,
/// keeping the protocol forward/backward compatible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamControl {
    /// A mirrored remote terminal changed grid size. The client resizes its
    /// mirror surface to match — the remote grid is authoritative for mirror
    /// geometry (local window/pane size never drives a mirror).
    Resize {
        /// Remote surface id. The client maps it to its local mirror surface id
        /// (workspace attach) or applies it to its sole mirror (surface attach).
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
}

/// Write a single framed message (`[tag][len BE][payload]`), then flush.
pub fn write_frame<W: Write>(w: &mut W, tag: StreamTag, payload: &[u8]) -> io::Result<()> {
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame payload exceeds u32"))?;
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

    #[test]
    fn mux_roundtrip() {
        let enc = encode_mux(42, b"hello");
        assert_eq!(&enc[..4], &42u32.to_be_bytes());
        let (sid, rest) = decode_mux(&enc).unwrap();
        assert_eq!(sid, 42);
        assert_eq!(rest, b"hello");
        // empty payload still carries the id.
        let empty = encode_mux(7, b"");
        let (sid2, rest2) = decode_mux(&empty).unwrap();
        assert_eq!(sid2, 7);
        assert!(rest2.is_empty());
    }

    #[test]
    fn decode_mux_rejects_truncated() {
        assert!(decode_mux(&[0u8, 1, 2]).is_none());
        assert!(decode_mux(&[]).is_none());
    }

    #[test]
    fn stream_control_resize_roundtrip() {
        let msg = StreamControl::Resize {
            surface_id: 7,
            cols: 157,
            rows: 45,
        };
        let s = serde_json::to_string(&msg).unwrap();
        // tagged on `event` = "resize" (snake_case).
        assert!(s.contains(r#""event":"resize""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn stream_control_ignores_foreign_events() {
        // Handshake/other events must NOT deserialize as a StreamControl variant —
        // the client relies on this to skip payloads it doesn't handle.
        for foreign in [
            r#"{"event":"attached","surface_id":1,"cols":80,"rows":24}"#,
            r#"{"event":"attach_error","reason":"x"}"#,
            r#"{"event":"force_detached"}"#,
        ] {
            assert!(serde_json::from_str::<StreamControl>(foreign).is_err());
        }
    }

    #[test]
    fn open_params_target_workspace_roundtrip() {
        let p = StreamOpenParams {
            proto: 1,
            target: None,
            target_workspace: Some(9),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: StreamOpenParams = serde_json::from_str(&s).unwrap();
        assert_eq!(back.target_workspace, Some(9));
        assert_eq!(back.target, None);
        // 구버전(필드 없음) 호환.
        let old: StreamOpenParams = serde_json::from_str(r#"{"proto":1}"#).unwrap();
        assert_eq!(old.target_workspace, None);
    }
}
