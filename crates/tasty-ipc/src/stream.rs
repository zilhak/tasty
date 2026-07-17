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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamControl {
    /// A mirrored remote terminal's grid settled at a new size. The client
    /// resizes its mirror surface to match. This is the **authoritative confirm**
    /// of the geometry the client requested via [`StreamControl::ClientResize`]:
    /// the remote PTY is the single source of truth for the real grid (it owns
    /// the reflow), and it echoes the settled size back here. The client applies
    /// its mirror grid **only** from this echo (never optimistically from the
    /// local pane), so mirror content is always replayed at the size the remote
    /// actually reflowed to — no desync.
    ///
    /// Direction: **server→client**.
    Resize {
        /// Remote surface id. The client maps it to its local mirror surface id
        /// (workspace attach) or applies it to its sole mirror (surface attach).
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
    /// A remote surface's busy/idle activity state (the same foreground-process
    /// heuristic `CoreState::refresh_busy_surfaces` already computes for the
    /// remote's own local terminals) flipped. The client applies `busy` to its
    /// mirror surface's own busy-state tracking (mirror terminals have no local
    /// PTY, so they can never compute this themselves — this push is the *only*
    /// source for mirror activity). Consumed by the workspace sidebar's "running"
    /// status dot (`busy_count`), which is otherwise blind to mirror workspaces.
    ///
    /// Pushed once per 1Hz busy-poll tick per occupied surface whose busy value
    /// actually changed since the last push (idempotent state, not a delta — a
    /// dropped/lagged frame self-heals on the next tick since the server always
    /// re-diffs from its live busy set, never from a client ack).
    ///
    /// Direction: **server→client**.
    Activity {
        /// Remote surface id, resolved the same way as [`StreamControl::Resize`].
        surface_id: u32,
        busy: bool,
    },
    /// The client (mirror side) requests the remote PTY be resized to the grid of
    /// its **local mirror pane**. Mirror geometry is **client-driven**: the pane
    /// the user placed the mirror in decides the grid, and the client pushes that
    /// intent here. The server resizes the real remote PTY (`Terminal::resize`),
    /// which reflows and echoes the settled size back as
    /// [`StreamControl::Resize`] — the client applies its mirror grid from that
    /// echo, not optimistically. Anchored on the **remote surface id** (mapped
    /// from the local mirror id before send). The occupying stream connection is
    /// the workspace's attach holder, so the connection itself proves the
    /// authority to drive geometry (ADR-0040 hard occupancy, ADR-0045).
    ///
    /// Direction: **client→server**. No explicit reply — the resulting
    /// [`StreamControl::Resize`] echo (present only when the grid actually
    /// changed) is the confirmation; an identical request is a no-op on the
    /// remote (`resize_grid` returns false → no echo).
    ClientResize {
        /// Remote surface id (the client maps its local mirror id to this before
        /// sending). The server resolves the enclosing workspace and verifies the
        /// requesting client is its attach holder before applying.
        surface_id: u32,
        cols: usize,
        rows: usize,
    },
    /// A structural change (split / new-tab / close / move) performed in a
    /// **mirror** workspace, forwarded to the remote (authoritative) instance so
    /// it runs there and spawns real PTYs — instead of leaking a local shell into
    /// the mirror. Anchored on **remote surface ids** (the only ids the client
    /// maps back to the remote): the server resolves pane/tab/workspace from its
    /// own tree. The occupying stream connection *is* the attach holder, so the
    /// connection itself proves the authority to mutate the workspace (ADR-0040
    /// hard occupancy).
    ///
    /// Direction: **client→server**. The server replies with a
    /// [`StreamControl::StructuralResult`] carrying the same `op_id`.
    StructuralOp {
        /// Client-assigned monotonic id, echoed back in the result so the client
        /// can correlate the reply (and toast on failure).
        op_id: u64,
        op: StructuralOp,
    },
    /// Result of a forwarded [`StreamControl::StructuralOp`]. `ok=false` carries a
    /// `reason` — most notably a **remote-unsupported surface kind** (e.g. a
    /// plugin `markdown` kind present locally but not on the remote host, whose
    /// kind registry is the authority for what it can create). The client shows a
    /// failure toast; neither side changes structure on a rejected op.
    ///
    /// Direction: **server→client**.
    StructuralResult {
        op_id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Full re-sync of a mirror workspace's structure after a forwarded
    /// [`StreamControl::StructuralOp`] succeeded on the remote (authoritative)
    /// instance — the reverse-reflection channel (3단계). Rather than a minimal
    /// per-surface diff (which would force the client to track remote pane/tab
    /// ids — it only maps *surfaces*, a 2단계 invariant), the server pushes the
    /// **entire** post-op workspace tree plus per-surface descriptors, and the
    /// client rebuilds its mirror with survivor terminals preserved (existing
    /// mirror grids keep their local ids → no scrollback loss). This covers
    /// split / new-tab / close-cascade / move-tab uniformly.
    ///
    /// `tree` / `surfaces` are the same shapes the handshake descriptor
    /// (`attached_workspace`) carries, so client-side `build_mirror_workspace`
    /// is reused verbatim. The client derives added/removed by diffing
    /// `surfaces`' remote ids against its `remote_to_local` map, so no explicit
    /// diff is carried.
    ///
    /// Direction: **server→client**. Pushed immediately after the
    /// [`StreamControl::StructuralResult`] (ok=true) for the op that changed the
    /// structure, so the client applies the (silent) success then re-syncs.
    StructuralDelta {
        /// Remote workspace id (the client maps it to its local mirror
        /// workspace; a mirror session hosts exactly one workspace so this is
        /// mainly for validation/diagnostics).
        workspace_id: u32,
        /// Post-op full workspace tree (`to_attach_tree_json`, same shape as the
        /// handshake `tree`).
        tree: serde_json::Value,
        /// Post-op per-surface descriptors (same shape as the handshake
        /// `surfaces`: `{remote_id, role, cols, rows}` for terminals /
        /// `{remote_id, role, kind}` for placeholders).
        surfaces: Vec<serde_json::Value>,
    },
}

/// The concrete structural operation carried by [`StreamControl::StructuralOp`].
/// Every variant is anchored on remote surface id(s); the server resolves the
/// enclosing pane/tab/workspace from its authoritative tree, so the client never
/// needs to track remote pane/tab ids (it only maps surfaces).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StructuralOp {
    /// Split a surface within its tab. Anchor = the surface being split.
    SplitSurface {
        surface_id: u32,
        direction: SplitAxis,
        /// New surface kind (`"terminal"` or a registered plugin kind).
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Split the pane containing the anchor surface.
    SplitPane {
        anchor_surface_id: u32,
        direction: SplitAxis,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Add a new tab to the pane containing the anchor surface.
    NewTab {
        anchor_surface_id: u32,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Close a surface (cascading up to tab/pane/workspace as usual).
    CloseSurface { surface_id: u32 },
    /// Close the tab containing the anchor surface.
    CloseTab { anchor_surface_id: u32 },
    /// Close the pane containing the anchor surface.
    ClosePane { anchor_surface_id: u32 },
    /// Reorder a tab within the pane containing the anchor surface.
    MoveTab {
        anchor_surface_id: u32,
        from_index: usize,
        to_index: usize,
    },
    /// Convert a surface to a different kind in place.
    ConvertSurface {
        surface_id: u32,
        #[serde(default = "default_terminal_kind")]
        surface_kind: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Move a live surface onto another surface's slot (both remote ids).
    MoveSurface {
        source_surface_id: u32,
        target_surface_id: u32,
    },
}

impl StructuralOp {
    /// The remote surface id this op is anchored on. The server uses it to locate
    /// the enclosing workspace and verify the requesting client is its attach
    /// holder before executing (only the occupier may mutate the workspace).
    pub fn anchor_surface_id(&self) -> u32 {
        match self {
            StructuralOp::SplitSurface { surface_id, .. }
            | StructuralOp::CloseSurface { surface_id }
            | StructuralOp::ConvertSurface { surface_id, .. } => *surface_id,
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            }
            | StructuralOp::NewTab {
                anchor_surface_id, ..
            }
            | StructuralOp::CloseTab { anchor_surface_id }
            | StructuralOp::ClosePane { anchor_surface_id }
            | StructuralOp::MoveTab {
                anchor_surface_id, ..
            } => *anchor_surface_id,
            StructuralOp::MoveSurface {
                source_surface_id, ..
            } => *source_surface_id,
        }
    }

    /// Return a copy with the anchor surface id replaced. The mirror client builds
    /// an op with a **local** anchor id (the only id it knows at the block point),
    /// then swaps in the mapped **remote** id before sending. For `MoveSurface`
    /// the anchor is the source; the target id is left untouched (both are remote
    /// ids the client already holds).
    pub fn with_anchor_surface_id(&self, remote: u32) -> StructuralOp {
        let mut cloned = self.clone();
        match &mut cloned {
            StructuralOp::SplitSurface { surface_id, .. }
            | StructuralOp::CloseSurface { surface_id }
            | StructuralOp::ConvertSurface { surface_id, .. } => *surface_id = remote,
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            }
            | StructuralOp::NewTab {
                anchor_surface_id, ..
            }
            | StructuralOp::CloseTab { anchor_surface_id }
            | StructuralOp::ClosePane { anchor_surface_id }
            | StructuralOp::MoveTab {
                anchor_surface_id, ..
            } => *anchor_surface_id = remote,
            StructuralOp::MoveSurface {
                source_surface_id, ..
            } => *source_surface_id = remote,
        }
        cloned
    }
}

/// Split direction carried over the wire. Maps to the host `SplitDirection` /
/// the IPC `"vertical"`/`"horizontal"` convention on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Vertical,
    Horizontal,
}

impl SplitAxis {
    /// The IPC/`SplitDirection` string form (`handle_split` parses this).
    pub fn as_ipc_str(self) -> &'static str {
        match self {
            SplitAxis::Vertical => "vertical",
            SplitAxis::Horizontal => "horizontal",
        }
    }
}

fn default_terminal_kind() -> String {
    "terminal".to_string()
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
    fn stream_control_activity_roundtrip() {
        let msg = StreamControl::Activity {
            surface_id: 9,
            busy: true,
        };
        let s = serde_json::to_string(&msg).unwrap();
        // tagged on `event` = "activity" (snake_case).
        assert!(s.contains(r#""event":"activity""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn stream_control_client_resize_roundtrip() {
        let msg = StreamControl::ClientResize {
            surface_id: 12,
            cols: 203,
            rows: 57,
        };
        let s = serde_json::to_string(&msg).unwrap();
        // tagged on `event` = "client_resize" (snake_case) — distinct from the
        // server→client "resize" so the two directions never collide.
        assert!(s.contains(r#""event":"client_resize""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn client_resize_and_resize_are_distinct_events() {
        // A ClientResize (client→server) must not deserialize as a Resize
        // (server→client) or vice-versa — the direction is carried by the tag.
        let client = serde_json::to_string(&StreamControl::ClientResize {
            surface_id: 1,
            cols: 80,
            rows: 24,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&client).unwrap(),
            StreamControl::ClientResize { .. }
        ));
        let server = serde_json::to_string(&StreamControl::Resize {
            surface_id: 1,
            cols: 80,
            rows: 24,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&server).unwrap(),
            StreamControl::Resize { .. }
        ));
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
    fn stream_control_structural_op_roundtrip() {
        let msg = StreamControl::StructuralOp {
            op_id: 42,
            op: StructuralOp::SplitSurface {
                surface_id: 7,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
        };
        let s = serde_json::to_string(&msg).unwrap();
        // outer tagged on `event`, inner op tagged on `kind`.
        assert!(s.contains(r#""event":"structural_op""#));
        assert!(s.contains(r#""kind":"split_surface""#));
        assert!(s.contains(r#""direction":"horizontal""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn stream_control_structural_op_all_kinds_roundtrip() {
        let ops = [
            StructuralOp::SplitPane {
                anchor_surface_id: 3,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({"a": 1}),
            },
            StructuralOp::NewTab {
                anchor_surface_id: 4,
                surface_kind: "markdown".to_string(),
                params: serde_json::json!({"file": "/x"}),
            },
            StructuralOp::CloseSurface { surface_id: 5 },
            StructuralOp::CloseTab {
                anchor_surface_id: 6,
            },
            StructuralOp::ClosePane {
                anchor_surface_id: 7,
            },
            StructuralOp::MoveTab {
                anchor_surface_id: 8,
                from_index: 0,
                to_index: 2,
            },
            StructuralOp::ConvertSurface {
                surface_id: 9,
                surface_kind: "image".to_string(),
                params: serde_json::json!({}),
            },
            StructuralOp::MoveSurface {
                source_surface_id: 10,
                target_surface_id: 11,
            },
        ];
        for op in ops {
            let msg = StreamControl::StructuralOp {
                op_id: 1,
                op: op.clone(),
            };
            let s = serde_json::to_string(&msg).unwrap();
            let back: StreamControl = serde_json::from_str(&s).unwrap();
            assert_eq!(back, msg, "roundtrip failed for {op:?}");
        }
    }

    #[test]
    fn structural_op_defaults_terminal_kind_and_empty_params() {
        // Client may omit surface_kind/params for a plain terminal split.
        let raw = r#"{"kind":"split_surface","surface_id":1,"direction":"vertical"}"#;
        let op: StructuralOp = serde_json::from_str(raw).unwrap();
        match op {
            StructuralOp::SplitSurface {
                surface_kind,
                params,
                ..
            } => {
                assert_eq!(surface_kind, "terminal");
                assert_eq!(params, serde_json::Value::Null);
            }
            _ => panic!("expected split_surface"),
        }
    }

    #[test]
    fn stream_control_structural_result_roundtrip() {
        // success (no reason) and failure (with reason).
        let ok = StreamControl::StructuralResult {
            op_id: 9,
            ok: true,
            reason: None,
        };
        let s = serde_json::to_string(&ok).unwrap();
        assert!(s.contains(r#""event":"structural_result""#));
        assert!(!s.contains("reason")); // skipped when None
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), ok);

        let fail = StreamControl::StructuralResult {
            op_id: 9,
            ok: false,
            reason: Some("unsupported kind: markdown".to_string()),
        };
        let s = serde_json::to_string(&fail).unwrap();
        assert!(s.contains("unsupported kind"));
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), fail);
    }

    #[test]
    fn stream_control_structural_delta_roundtrip() {
        let msg = StreamControl::StructuralDelta {
            workspace_id: 3,
            tree: serde_json::json!({
                "panes": [{ "id": 1, "tabs": [] }],
                "focused_pane": 1,
            }),
            surfaces: vec![
                serde_json::json!({"remote_id": 10, "role": "terminal", "cols": 80, "rows": 24}),
                serde_json::json!({"remote_id": 11, "role": "placeholder", "kind": "markdown"}),
            ],
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"structural_delta""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn structural_delta_not_confused_with_other_events() {
        // A StructuralDelta payload must not deserialize as Resize/StructuralOp/
        // StructuralResult (and vice versa) — the `event` tag keeps them distinct.
        let delta = serde_json::to_string(&StreamControl::StructuralDelta {
            workspace_id: 1,
            tree: serde_json::Value::Null,
            surfaces: vec![],
        })
        .unwrap();
        match serde_json::from_str::<StreamControl>(&delta).unwrap() {
            StreamControl::StructuralDelta { workspace_id, .. } => assert_eq!(workspace_id, 1),
            other => panic!("expected StructuralDelta, got {other:?}"),
        }
        // A resize event stays a resize.
        assert!(matches!(
            serde_json::from_str::<StreamControl>(
                r#"{"event":"resize","surface_id":1,"cols":80,"rows":24}"#
            )
            .unwrap(),
            StreamControl::Resize { .. }
        ));
    }

    #[test]
    fn structural_op_anchor_surface_id() {
        assert_eq!(
            StructuralOp::CloseSurface { surface_id: 5 }.anchor_surface_id(),
            5
        );
        assert_eq!(
            StructuralOp::ClosePane {
                anchor_surface_id: 9
            }
            .anchor_surface_id(),
            9
        );
        assert_eq!(
            StructuralOp::MoveSurface {
                source_surface_id: 3,
                target_surface_id: 4
            }
            .anchor_surface_id(),
            3
        );
    }

    #[test]
    fn structural_op_with_anchor_surface_id() {
        // Local anchor swapped for the remote id before send.
        let local = StructuralOp::SplitPane {
            anchor_surface_id: 5, // local mirror id
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let remote = local.with_anchor_surface_id(100);
        assert_eq!(remote.anchor_surface_id(), 100);
        // MoveSurface swaps source, keeps target.
        let mv = StructuralOp::MoveSurface {
            source_surface_id: 1,
            target_surface_id: 2,
        }
        .with_anchor_surface_id(9);
        match mv {
            StructuralOp::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => {
                assert_eq!(source_surface_id, 9);
                assert_eq!(target_surface_id, 2);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn split_axis_ipc_str() {
        assert_eq!(SplitAxis::Vertical.as_ipc_str(), "vertical");
        assert_eq!(SplitAxis::Horizontal.as_ipc_str(), "horizontal");
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
