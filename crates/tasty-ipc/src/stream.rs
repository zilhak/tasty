//! stream.open을 첫 줄로 보내 JSON-RPC 연결을 바이너리 스트림으로 전환한다.
//! 프레임은 [tag:u8][len:u32 BE][payload] 형식이다. 점유·mirror 처리는 상위 계층이 맡는다.

mod read_failure;

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use read_failure::ReadFailure;
// attach Control 메시지·capability 이름·descriptor 키는 tasty-attach-protocol이 정의하고 이 경로로 다시 내보낸다.
pub use tasty_attach_protocol::{
    AttachedWorkspaceDescriptor, AttentionKindWire, DESCRIPTOR_CAPABILITIES, DESCRIPTOR_PTY_INPUT,
    FORCE_DETACH_WORKSPACE_GONE, FORCE_DETACHED_EVENT, ForceDetachNotice, ForwardOrigin,
    INHERIT_CWD_CAPABILITY, INPUT_ACK_CAPABILITY, InheritCwd, NOTIFICATION_SOUND_CAPABILITY,
    PTY_INPUT_VT, PTY_INPUT_WIN32_CONSOLE, RESIZE_ACK_CAPABILITY, RESIZE_ACK_VERSION,
    STRUCTURAL_REASON_PATH_NOT_FOUND_PREFIX, STRUCTURAL_REASON_RESTORE_EMPTY, SplitAxis,
    StreamControl, StructuralOp, WORKSPACE_CLOSE_CAPABILITY, descriptor_wants_win32_console_input,
    force_detach_notice, local_pty_input,
};

/// Method name whose presence as the first JSON-RPC line upgrades a connection
/// to the streaming channel.
pub const STREAM_OPEN_METHOD: &str = "stream.open";

/// handshake에서 동일한 값인지 비교하는 프로토콜 버전. 변경하면 기존 peer 연결이 거절된다.
/// 추가 기능은 capability와 ClientLossNotify 같은 별도 선언으로 협상한다.
pub const STREAM_PROTO: u32 = 1;

/// Frame header length: 1-byte tag + 4-byte big-endian payload length.
pub const FRAME_HEADER_LEN: usize = 5;

/// 프레임 payload 상한(1 MiB). 초과하면 할당 전에 거절하고 연결을 끝낸다.
pub const MAX_FRAME_LEN: u32 = 1 << 20;

/// Idle interval between application-level [`StreamTag::Ping`] heartbeats sent
/// by either peer's write side while no other frame has gone out. Real traffic
/// (Data/Control) counts as liveness too — a peer only falls back to sending a
/// bare Ping once this long has passed with nothing else to send.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Default socket read timeout applied to attach streams. Six heartbeats' worth
/// of slack so transient jitter or a brief network stall doesn't trip a false
/// disconnect — a peer is declared dead only after missing several heartbeats
/// in a row.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

/// New attach connections read their read timeout from this value once. Clones
/// share it, so a settings change reaches later connections while sockets that
/// are already open keep the timeout they were given.
#[derive(Clone, Debug)]
pub struct SharedHeartbeatTimeout(Arc<AtomicU64>);

impl Default for SharedHeartbeatTimeout {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(HEARTBEAT_TIMEOUT.as_secs())))
    }
}

impl SharedHeartbeatTimeout {
    pub fn get(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, timeout: Duration) {
        self.0.store(timeout.as_secs(), Ordering::Relaxed);
    }
}

/// Frame type tag (first byte of every frame).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum StreamTag {
    /// PTY 출력 또는 echo 등의 바이트 payload.
    Data = 0,
    /// handshake와 세션 제어용 UTF-8 JSON.
    Control = 1,
    /// Application-level keepalive. Sent with an empty payload by either peer's
    /// write side when idle for [`HEARTBEAT_INTERVAL`]; receiving *any* frame
    /// (including this one) resets the reader's [`HEARTBEAT_TIMEOUT`] socket
    /// read timeout, so no explicit handling is needed on receipt.
    Ping = 2,
    /// Graceful close signal (empty payload), either direction.
    Detach = 3,
    /// Opaque plugin egui-mesh frame bytes, chunked (`crate::mesh_stream`). Kept
    /// distinct from [`StreamTag::Data`] (which carries PTY bytes, optionally
    /// surface-muxed) so a mesh consumer never has to disambiguate mesh chunks
    /// from terminal output at the demux layer — attach mesh mirror
    /// (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
    MeshData = 4,
}

impl StreamTag {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Data),
            1 => Some(Self::Control),
            2 => Some(Self::Ping),
            3 => Some(Self::Detach),
            4 => Some(Self::MeshData),
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
    /// attach할 surface ID. 지정하면 handshake 뒤 점유와 초기 snapshot 전송을 요청한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
    /// attach할 workspace ID. 터미널별 Data에는 surface ID prefix가 붙는다.
    /// target과 함께 지정하면 서버가 거절한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_workspace: Option<u32>,
    /// bulk 파일 연결을 묶을 workspace ID. 대화형 holder가 되지 않으며 Data는 파일 청크다.
    /// 서버는 해당 workspace에 활성 holder가 있어야 전송을 허용한다. 다른 target과 상호 배타다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bulk_workspace: Option<u32>,
}

/// workspace Data에 4바이트 big-endian surface ID를 붙인다. 단일 surface 연결에는 쓰지 않는다.
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

/// bulk 파일 전송(docs/dev-guide/attach-behavior.md#커스텀-이벤트-확장-streamcontrol-밖-raw-json-event-태그)의 `Data` 프레임 sub-header 길이 = `[transfer_id: u64 BE][seq: u32 BE]`.
pub const BULK_CHUNK_HEADER_LEN: usize = 12;

/// bulk 파일 청크 `Data` 프레임 인코딩. 페이로드 앞에 12바이트 binary sub-header
/// (`[transfer_id: u64 BE][seq: u32 BE]`)를 붙여 raw 파일 바이트를 실어 나른다.
/// `encode_mux`(surface 다중화)와 달리 transfer/seq 를 실으며, base64 를 쓰지 않는다.
/// `seq` 는 진단·검증용(TCP 는 연결당 순서 보장이라 재정렬에 쓰지 않는다 — 캡처
/// 업로드와 동일 근거).
pub fn encode_bulk_chunk(transfer_id: u64, seq: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(BULK_CHUNK_HEADER_LEN + bytes.len());
    out.extend_from_slice(&transfer_id.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

/// [`encode_bulk_chunk`] 의 역연산. 12바이트(sub-header) 미만이면 `None`(잘린 프레임).
/// 반환: `(transfer_id, seq, 파일 바이트 슬라이스)`.
pub fn decode_bulk_chunk(buf: &[u8]) -> Option<(u64, u32, &[u8])> {
    if buf.len() < BULK_CHUNK_HEADER_LEN {
        return None;
    }
    let transfer_id = u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]);
    let seq = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
    Some((transfer_id, seq, &buf[BULK_CHUNK_HEADER_LEN..]))
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

/// 헤더와 payload를 한 버퍼에 모아 write_all을 한 번 호출한 뒤 flush한다.
/// 소켓의 TCP_NODELAY와 함께 작은 프레임의 분할 전송 지연을 줄인다.
/// write_all 내부에서 부분 쓰기를 재시도할 수 있어 단일 syscall을 보장하지는 않는다.
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
    let mut buf = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    buf.push(tag as u8);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    w.write_all(&buf)?;
    w.flush()
}

/// EOF·알 수 없는 tag·초과 길이·읽기 timeout은 오류다.
/// 헤더나 payload를 일부 읽은 뒤 오류가 나면 같은 연결에서 재시도하지 않는다.
/// read_exact가 소비한 바이트를 복원할 수 없어 프레임 경계가 어긋날 수 있다.
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

    /// 전체 입력을 받는 시험 writer에서 헤더·payload를 나눠 쓰지 않는지 확인한다.
    #[test]
    fn write_frame_emits_one_write_call() {
        struct CountingWriter {
            writes: usize,
            buf: Vec<u8>,
        }
        impl Write for CountingWriter {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                self.writes += 1;
                self.buf.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        for payload in [Vec::new(), b"hi".to_vec(), vec![7u8; 4096]] {
            let mut w = CountingWriter {
                writes: 0,
                buf: Vec::new(),
            };
            write_frame(&mut w, StreamTag::Data, &payload).unwrap();
            assert_eq!(
                w.writes,
                1,
                "payload {} 바이트가 {} 번의 write 로 쪼개졌다",
                payload.len(),
                w.writes
            );
            assert_eq!(w.buf.len(), FRAME_HEADER_LEN + payload.len());
        }
    }

    #[test]
    fn heartbeat_timeout_has_jitter_margin_over_interval() {
        // 한 번의 지연으로 끊기지 않도록 heartbeat보다 충분히 긴 timeout을 유지한다.
        assert!(HEARTBEAT_TIMEOUT >= HEARTBEAT_INTERVAL * 2);
        assert_eq!(HEARTBEAT_TIMEOUT, HEARTBEAT_INTERVAL * 6);
    }

    #[test]
    fn ping_frame_has_empty_payload_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, StreamTag::Ping, &[]).unwrap();
        let mut cur = Cursor::new(buf);
        let frame = read_frame(&mut cur).unwrap();
        assert_eq!(frame.tag, StreamTag::Ping);
        assert!(frame.payload.is_empty());
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
    fn bulk_chunk_roundtrip() {
        let enc = encode_bulk_chunk(0x0102_0304_0506_0708, 42, b"payload");
        assert_eq!(&enc[..8], &0x0102_0304_0506_0708u64.to_be_bytes());
        assert_eq!(&enc[8..12], &42u32.to_be_bytes());
        let (tid, seq, rest) = decode_bulk_chunk(&enc).unwrap();
        assert_eq!(tid, 0x0102_0304_0506_0708);
        assert_eq!(seq, 42);
        assert_eq!(rest, b"payload");
        let empty = encode_bulk_chunk(7, 0, b"");
        let (tid2, seq2, rest2) = decode_bulk_chunk(&empty).unwrap();
        assert_eq!(tid2, 7);
        assert_eq!(seq2, 0);
        assert!(rest2.is_empty());
    }

    #[test]
    fn decode_bulk_chunk_rejects_truncated() {
        assert!(decode_bulk_chunk(&[]).is_none());
        assert!(decode_bulk_chunk(&[0u8; 11]).is_none());
        assert!(decode_bulk_chunk(&[0u8; 12]).is_some());
    }

    #[test]
    fn open_params_target_workspace_roundtrip() {
        let p = StreamOpenParams {
            proto: 1,
            target: None,
            target_workspace: Some(9),
            bulk_workspace: None,
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: StreamOpenParams = serde_json::from_str(&s).unwrap();
        assert_eq!(back.target_workspace, Some(9));
        assert_eq!(back.target, None);
        assert_eq!(back.bulk_workspace, None);
        let old: StreamOpenParams = serde_json::from_str(r#"{"proto":1}"#).unwrap();
        assert_eq!(old.target_workspace, None);
        assert_eq!(old.bulk_workspace, None);
    }

    #[test]
    fn open_params_bulk_workspace_roundtrip_and_backward_compat() {
        let p = StreamOpenParams {
            proto: 1,
            target: None,
            target_workspace: None,
            bulk_workspace: Some(7),
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""bulk_workspace":7"#));
        let back: StreamOpenParams = serde_json::from_str(&s).unwrap();
        assert_eq!(back.bulk_workspace, Some(7));
        assert_eq!(back.target, None);
        assert_eq!(back.target_workspace, None);
        let old: StreamOpenParams =
            serde_json::from_str(r#"{"proto":1,"target_workspace":3}"#).unwrap();
        assert_eq!(old.bulk_workspace, None);
        assert_eq!(old.target_workspace, Some(3));
    }

    /// 추가 Loss 기능 때문에 기존 peer와의 handshake 버전이 바뀌지 않았는지 확인한다.
    #[test]
    fn adding_a_control_variant_does_not_move_the_handshake_version() {
        assert_eq!(STREAM_PROTO, 1);
    }
}
