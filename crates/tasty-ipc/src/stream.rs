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
use std::time::Duration;

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

/// Idle interval between application-level [`StreamTag::Ping`] heartbeats sent
/// by either peer's write side while no other frame has gone out. Real traffic
/// (Data/Control) counts as liveness too — a peer only falls back to sending a
/// bare Ping once this long has passed with nothing else to send.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Socket read timeout applied to attach streams. Four heartbeats' worth of
/// slack so transient jitter/scheduling delay doesn't trip a false disconnect —
/// a peer is declared dead only after missing several heartbeats in a row.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(20);

/// Frame type tag (first byte of every frame).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum StreamTag {
    /// Raw bytes (future PTY output / echo payload).
    Data = 0,
    /// UTF-8 JSON control message (handshake ack; future resize/detach metadata).
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
    /// (`.claude-workspace/todo/15-attach-protocol-mesh-messages.md`).
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
    /// bulk 파일 전송 전용 연결(ADR-0054). `Some(ws)` 이면 이 연결은 대화형 attach 를
    /// 하지 않고(= workspace holder 가 되지 않음), 그 `Data` 프레임을 PTY 입력이 아니라
    /// **파일 청크**(`decode_bulk_chunk`)로 분류하도록 서버가 이 연결을 bulk 로 태깅한다.
    /// 결속 workspace(`ws`)는 저장·인가의 대상: 서버는 이 ws 에 활성 holder 가 존재할
    /// 때만 전송을 수락한다(전용 연결 자체는 holder 가 아니므로 별도 결속 필요 —
    /// 조사 §6). `target`/`target_workspace` 와 상호배타(bulk 연결은 mirror 하지 않는다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bulk_workspace: Option<u32>,
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

/// bulk 파일 전송(ADR-0054)의 `Data` 프레임 sub-header 길이 = `[transfer_id: u64 BE][seq: u32 BE]`.
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
    /// bulk 파일 전송(ADR-0054)의 control-plane 시작 메시지. 전용 bulk 연결에서
    /// 실제 파일 바이트(`Data` 프레임, [`encode_bulk_chunk`])에 앞서 파일명·총 크기를
    /// 알린다. 서버는 `total_size` 를 사전 용량 승인(07)의 입력으로 쓰고, `transfer_id`
    /// 단위로 청크를 누적한다. 저장 dir 결정·경로 회신은 `commit` 에서 확정.
    ///
    /// Direction: **client→server**.
    BulkBegin {
        transfer_id: u64,
        filename: String,
        total_size: u64,
    },
    /// bulk 전송 완료 신호. 서버는 누적 바이트를 파일로 저장 확정하고
    /// [`StreamControl::BulkResult`] 로 원격 절대경로(또는 실패사유)를 회신한다.
    ///
    /// Direction: **client→server**.
    BulkCommit { transfer_id: u64 },
    /// bulk 전송 결과. `ok=true` 면 `path` 에 원격 파일시스템 절대경로, `ok=false` 면
    /// `reason` 에 실패사유(용량 초과·미인가·저장 실패 등). 소비자(08/09)가 이 경로를
    /// 대화형 스트림에 삽입하거나 진행 UI 에 표시한다.
    ///
    /// Direction: **server→client**.
    BulkResult {
        transfer_id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The attach client's mesh mirror pane pushes the plugin egui-mesh context
    /// (geometry/scale/theme/focus) it wants the remote surface driven with —
    /// mirrors `SurfaceSetContextParams` minus the per-frame input batch (that's
    /// [`StreamControl::MeshInput`]). Sending this **is** the subscribe signal
    /// (no separate handshake capability negotiation, TODO 15 decision, mirrors
    /// the existing [`StreamControl::ClientResize`] "request itself declares
    /// intent" pattern): the server activates mesh forwarding for `surface_id`
    /// the first time it sees one of these, and re-drives the remote plugin's
    /// `set_context` any time geometry/theme/focus changes thereafter.
    ///
    /// Direction: **client→server**.
    MeshContext {
        /// Remote surface id (client-mapped, like every other mirror message).
        surface_id: u32,
        /// Physical pixel width of the client's local mirror pane.
        width_px: u32,
        /// Physical pixel height of the client's local mirror pane.
        height_px: u32,
        /// Logical→physical scale (egui `ScreenDescriptor.pixels_per_point`).
        pixels_per_point: f32,
        /// The client's own resolved theme — the mirror should visually match
        /// what the *attach client* is displaying, not the (possibly headless,
        /// possibly differently-themed) server.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        /// Whether the local mirror pane currently has keyboard focus.
        #[serde(default)]
        focused: bool,
    },
    /// Local input captured over the mesh mirror pane, forwarded verbatim as a
    /// `RawInputWire` batch (the same wire shape the host already sends plugins
    /// via `SurfaceSetContextParams.raw_input` — no new event schema). Includes
    /// `PointerGone`/focus/modifier state, not just discrete clicks/keys, so the
    /// remote plugin's hover/focus state never drifts from the client's.
    ///
    /// Direction: **client→server**. No reply — the resulting
    /// [`StreamControl`]`::Mesh*` data push (once the remote repaints) is the
    /// only feedback, same as local (same-process) egui-mesh input forwarding.
    MeshInput {
        surface_id: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    },
    /// The client requests the remote re-send **all** of a mesh surface's
    /// current texture state as a `full_textures=true` frame — sent when the
    /// client detects a `frame_seq` chain break (fresh subscribe, or a gap after
    /// reconnect: the server's `SharedBuffer` poll only ever sees the *latest*
    /// generation, so a client that missed intermediate texture deltas has no
    /// other way to recover a consistent state). The server answers by setting
    /// `SurfaceSetContextParams.need_full_textures = true` on its next forward to
    /// the plugin.
    ///
    /// Direction: **client→server**. No explicit ack — the next
    /// [`StreamTag::MeshData`] chunk sequence for this `surface_id` arriving with
    /// `full_textures = true` (carried in the chunk header,
    /// `mesh_stream::MeshChunkMeta`) is the confirmation.
    MeshFullResendRequest { surface_id: u32 },
    /// The requested `surface_id` in a [`StreamControl::MeshContext`] is not a
    /// mesh-mirrorable surface on the remote (not found, not a bundled
    /// egui-mesh-whitelisted kind, or the surface's plugin isn't running) — a
    /// **one-shot explicit error**, mirroring the existing
    /// `execute_forwarded_structural_op` `ok:false`+`reason` convention (TODO 15
    /// decision: explicit failure over silent drop, so the client never waits
    /// forever for mesh data that will never arrive).
    ///
    /// Direction: **server→client**.
    MeshError { surface_id: u32, reason: String },
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
///
/// Deliberate design: if a socket read timeout (heartbeat protocol) fires
/// while only part of the 5-byte header (or payload) has been read, this
/// propagates that `Err` immediately rather than retrying — any bytes already
/// consumed by the interrupted `read_exact` are discarded, so a retry would
/// desync from the frame boundary. Every read loop that calls this (server,
/// GUI client, CLI client) treats *all* `Err` results — EOF, malformed frame,
/// or a mid-frame timeout alike — as an unconditional disconnect, so no caller
/// ever tries to resume a torn frame on the same connection.
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
    fn heartbeat_timeout_has_jitter_margin_over_interval() {
        // timeout 이 interval 보다 넉넉히 커야 한다 — 한 heartbeat 를 놓쳐도(스케줄링
        // 지연/일시적 혼잡) 바로 오탐 disconnect 로 이어지지 않게. 최소 2 회분 이상의
        // 여유(문서화된 설계는 4 배).
        assert!(HEARTBEAT_TIMEOUT >= HEARTBEAT_INTERVAL * 2);
        assert_eq!(HEARTBEAT_TIMEOUT, HEARTBEAT_INTERVAL * 4);
    }

    #[test]
    fn ping_frame_has_empty_payload_roundtrip() {
        // heartbeat 로 실제 보내는 형태(빈 payload)가 그대로 왕복되는지 — 태그 자체는
        // frame_roundtrip 에서 이미 검증하지만, 여기선 heartbeat 가 실제로 쓰는 정확한
        // 모양(빈 payload)만 별도로 못박아 둔다.
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
    fn bulk_chunk_roundtrip() {
        let enc = encode_bulk_chunk(0x0102_0304_0506_0708, 42, b"payload");
        // sub-header: 8 bytes transfer_id BE + 4 bytes seq BE.
        assert_eq!(&enc[..8], &0x0102_0304_0506_0708u64.to_be_bytes());
        assert_eq!(&enc[8..12], &42u32.to_be_bytes());
        let (tid, seq, rest) = decode_bulk_chunk(&enc).unwrap();
        assert_eq!(tid, 0x0102_0304_0506_0708);
        assert_eq!(seq, 42);
        assert_eq!(rest, b"payload");
        // empty payload still carries the full header.
        let empty = encode_bulk_chunk(7, 0, b"");
        let (tid2, seq2, rest2) = decode_bulk_chunk(&empty).unwrap();
        assert_eq!(tid2, 7);
        assert_eq!(seq2, 0);
        assert!(rest2.is_empty());
    }

    #[test]
    fn decode_bulk_chunk_rejects_truncated() {
        // anything shorter than the 12-byte sub-header is a torn frame.
        assert!(decode_bulk_chunk(&[]).is_none());
        assert!(decode_bulk_chunk(&[0u8; 11]).is_none());
        // exactly 12 bytes = valid header, empty payload.
        assert!(decode_bulk_chunk(&[0u8; 12]).is_some());
    }

    #[test]
    fn stream_control_bulk_begin_commit_result_roundtrip() {
        let begin = StreamControl::BulkBegin {
            transfer_id: 9,
            filename: "img.png".to_string(),
            total_size: 123_456,
        };
        let s = serde_json::to_string(&begin).unwrap();
        assert!(s.contains(r#""event":"bulk_begin""#));
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), begin);

        let commit = StreamControl::BulkCommit { transfer_id: 9 };
        let s = serde_json::to_string(&commit).unwrap();
        assert!(s.contains(r#""event":"bulk_commit""#));
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), commit);

        // success (path, no reason) and failure (reason, no path).
        let ok = StreamControl::BulkResult {
            transfer_id: 9,
            ok: true,
            path: Some("/home/u/.tasty/transfers/img.png".to_string()),
            reason: None,
        };
        let s = serde_json::to_string(&ok).unwrap();
        assert!(s.contains(r#""event":"bulk_result""#));
        assert!(!s.contains("reason")); // skipped when None
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), ok);

        let fail = StreamControl::BulkResult {
            transfer_id: 9,
            ok: false,
            path: None,
            reason: Some("bound workspace has no active holder".to_string()),
        };
        let s = serde_json::to_string(&fail).unwrap();
        assert!(!s.contains("path")); // skipped when None
        assert!(s.contains("no active holder"));
        assert_eq!(serde_json::from_str::<StreamControl>(&s).unwrap(), fail);
    }

    #[test]
    fn bulk_events_are_distinct_from_capture_and_structural() {
        // A bulk_begin must not be misread as a foreign event and vice-versa —
        // the `event` tag keeps the mid-session control messages disjoint.
        let begin = serde_json::to_string(&StreamControl::BulkBegin {
            transfer_id: 1,
            filename: "x".to_string(),
            total_size: 0,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&begin).unwrap(),
            StreamControl::BulkBegin { .. }
        ));
        // The (03) capture-upload events are NOT StreamControl variants — they must
        // still fail to parse as one (bulk added no accidental collision).
        for capture in [
            r#"{"event":"capture_chunk","upload_id":1,"seq":0,"total":1,"data_b64":"AA=="}"#,
            r#"{"event":"capture_commit","upload_id":1,"file_name":"x.png"}"#,
        ] {
            assert!(serde_json::from_str::<StreamControl>(capture).is_err());
        }
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
    fn stream_control_mesh_context_roundtrip() {
        use tasty_plugin_protocol::protocol::ThemeWire;
        use tasty_type_appearance::theme::ThemeColors;

        // raw JSON 문자열로 ThemeColors 를 만든다(44필드라 `json!` 매크로가 재귀
        // 한계에 걸림, `set_context_theme_snapshot_round_trips` 의 패턴 재사용).
        // 값 자체는 무관 — round-trip 동일성만 본다.
        const COLORS_JSON: &str = r##"{
            "crust":"#11111b","mantle":"#181825","base":"#1e1e2e","surface0":"#313244",
            "surface1":"#45475a","surface2":"#585b70","overlay0":"#6c7086","overlay1":"#7f849c",
            "overlay2":"#9399b2","text":"#cdd6f4","subtext1":"#bac2de","subtext0":"#a6adc8",
            "placeholder":"#9399b2","blue":"#89b4fa","green":"#a6e3a1","red":"#f38ba8",
            "yellow":"#f9e2af","peach":"#fab387","mauve":"#cba6f7","teal":"#94e2d5",
            "sky":"#89dceb","lavender":"#b4befe","flamingo":"#f2cdcd","pink":"#f5c2e7",
            "maroon":"#eba0ac","rosewater":"#f5e0dc","selection_bg":"#585b70",
            "vi_cursor_bg":"#f9e2af","search_match_bg":"#f9e2af","search_match_active_bg":"#fab387",
            "ansi_black":"#45475a","ansi_red":"#f38ba8","ansi_green":"#a6e3a1","ansi_yellow":"#f9e2af",
            "ansi_blue":"#89b4fa","ansi_magenta":"#f5c2e7","ansi_cyan":"#94e2d5","ansi_white":"#bac2de",
            "ansi_bright_black":"#585b70","ansi_bright_red":"#f38ba8","ansi_bright_green":"#a6e3a1",
            "ansi_bright_yellow":"#f9e2af","ansi_bright_blue":"#89b4fa","ansi_bright_magenta":"#f5c2e7",
            "ansi_bright_cyan":"#94e2d5","ansi_bright_white":"#a6adc8"
        }"##;
        let colors: ThemeColors = serde_json::from_str(COLORS_JSON).unwrap();

        let msg = StreamControl::MeshContext {
            surface_id: 5,
            width_px: 800,
            height_px: 600,
            pixels_per_point: 2.0,
            theme: Some(ThemeWire {
                colors,
                is_light: false,
                ui_zoom: 1.0,
            }),
            focused: true,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"mesh_context""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);

        // theme 없이도(headless-only, 클라이언트 테마 미확정 등) round-trip.
        let no_theme = StreamControl::MeshContext {
            surface_id: 6,
            width_px: 100,
            height_px: 100,
            pixels_per_point: 1.0,
            theme: None,
            focused: false,
        };
        let s2 = serde_json::to_string(&no_theme).unwrap();
        assert!(!s2.contains("theme"));
        assert_eq!(
            serde_json::from_str::<StreamControl>(&s2).unwrap(),
            no_theme
        );
    }

    #[test]
    fn stream_control_mesh_input_roundtrip() {
        use tasty_plugin_protocol::protocol::{RawInputEventWire, RawInputWire};

        let msg = StreamControl::MeshInput {
            surface_id: 8,
            input: RawInputWire {
                time: Some(1.5),
                focused: true,
                modifiers: Default::default(),
                events: vec![
                    RawInputEventWire::PointerMoved { x: 10.0, y: 20.0 },
                    RawInputEventWire::PointerGone,
                ],
            },
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"mesh_input""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn stream_control_mesh_full_resend_request_roundtrip() {
        let msg = StreamControl::MeshFullResendRequest { surface_id: 3 };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"mesh_full_resend_request""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn stream_control_mesh_error_roundtrip() {
        let msg = StreamControl::MeshError {
            surface_id: 11,
            reason: "surface is not egui-mesh whitelisted".to_string(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains(r#""event":"mesh_error""#));
        let back: StreamControl = serde_json::from_str(&s).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn mesh_events_are_distinct_from_each_other_and_existing_events() {
        // event tag 가 서로 다른 mesh variant 로 오인식되지 않는지 + 기존 이벤트와도
        // 섞이지 않는지.
        let ctx = serde_json::to_string(&StreamControl::MeshContext {
            surface_id: 1,
            width_px: 1,
            height_px: 1,
            pixels_per_point: 1.0,
            theme: None,
            focused: false,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&ctx).unwrap(),
            StreamControl::MeshContext { .. }
        ));
        let resend =
            serde_json::to_string(&StreamControl::MeshFullResendRequest { surface_id: 1 }).unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&resend).unwrap(),
            StreamControl::MeshFullResendRequest { .. }
        ));
        // 기존 이벤트(resize)가 mesh variant 로 잘못 파싱되지 않는다.
        let resize = serde_json::to_string(&StreamControl::Resize {
            surface_id: 1,
            cols: 80,
            rows: 24,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<StreamControl>(&resize).unwrap(),
            StreamControl::Resize { .. }
        ));
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
        // 구버전(필드 없음) 호환.
        let old: StreamOpenParams = serde_json::from_str(r#"{"proto":1}"#).unwrap();
        assert_eq!(old.target_workspace, None);
        assert_eq!(old.bulk_workspace, None);
    }

    #[test]
    fn open_params_bulk_workspace_roundtrip_and_backward_compat() {
        // bulk 필드 추가 = 하위호환: 새 필드로 직렬화한 payload 도, 옛 payload 도 모두 파싱.
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
        // bulk_workspace 를 모르는 옛 서버가 만든 payload(필드 없음)도 그대로 수용.
        let old: StreamOpenParams =
            serde_json::from_str(r#"{"proto":1,"target_workspace":3}"#).unwrap();
        assert_eq!(old.bulk_workspace, None);
        assert_eq!(old.target_workspace, Some(3));
    }
}
