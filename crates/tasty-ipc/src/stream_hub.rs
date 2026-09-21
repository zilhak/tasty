//! Server-side streaming push registry for the attach/detach feature (step 1).
//!
//! Holds one bounded push sink per upgraded stream connection so the main loop
//! can push frames to a specific client *without ever blocking* (slow clients
//! drop frames, then get disconnected). The IPC accept threads register and
//! unregister sinks; the main loop pushes and drains inbound frames.
//!
//! Security (decisions.md #5): the streaming channel carries no token of its own
//! — trust is delegated to SSH + 127.0.0.1 loopback. No auth layer here.
//!
//! See `docs/dev-guide/attach-behavior.md` ("SSH 터널", "IPC 표면").
//!
//! 이 모듈은 전송 수단(TCP)을 모른다 — sink 는 `std::sync::mpsc` 채널이고, 소켓에
//! 쓰고 읽는 쪽(accept 스레드)은 본체 adapter(`src/adapters/production/tcp_ipc_server.rs`)
//! 에 있다. 그래서 본체 core 가 adapter 를 거치지 않고 이 허브를 쓸 수 있다
//! (docs/adr/0350-the-stream-hub-lives-in-the-ipc-crate.md).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use crate::server::IpcWaker;
use crate::stream::StreamFrame;

/// Bounded capacity of each per-client push sink. A client whose sink fills up
/// has frames dropped — the main loop never blocks on a slow consumer.
const SINK_CAP: usize = 1024;

/// Consecutive dropped frames after which a lagging client is force-disconnected.
const LAG_LIMIT: u32 = 64;

/// Identifier of an upgraded streaming connection.
pub type StreamClientId = u32;

/// One message routed from a stream connection to the main loop.
pub enum StreamInbound {
    /// A frame received from a stream client.
    Frame {
        client_id: StreamClientId,
        frame: StreamFrame,
    },
    /// A stream client requested attach to a surface (`stream.open` with a
    /// `target`). The main loop acquires the lock, taps output, and pushes the
    /// initial snapshot (attach/detach step 4). Routed via the inbound channel
    /// because the accept thread cannot touch the engine (main-loop owned).
    AttachRequest {
        client_id: StreamClientId,
        target_surface_id: u32,
    },
    /// A stream client requested attach to a whole workspace (`stream.open` with
    /// `target_workspace`, attach/detach step 6). The main loop mirrors every
    /// terminal surface in the workspace and hides non-terminals (decision 3).
    AttachWorkspaceRequest {
        client_id: StreamClientId,
        target_workspace_id: u32,
    },
    /// A stream client's connection closed (EOF / read error / detach). The main
    /// loop releases any attach locks that client held (attach/detach step 3).
    Disconnected { client_id: StreamClientId },
}

/// Classified inbound messages for one `pump_inbound` drain (attach/detach step
/// 4). The main loop applies each to the engine; classification lives here
/// (no engine access) while interpretation lives in the main loop.
#[derive(Default)]
pub struct PumpOutcome {
    /// Clients whose connections closed — release their attach locks.
    pub disconnected: Vec<StreamClientId>,
    /// `(client_id, target_surface_id)` attach requests.
    pub attach_requests: Vec<(StreamClientId, u32)>,
    /// `(client_id, target_workspace_id)` workspace attach requests (step 6).
    pub workspace_attach_requests: Vec<(StreamClientId, u32)>,
    /// `(client_id, bytes)` input data frames — route to the held surface's PTY.
    /// In workspace mode the bytes are surface-prefixed (`decode_mux`); the main
    /// loop demuxes based on whether the client holds a workspace.
    pub input_frames: Vec<(StreamClientId, Vec<u8>)>,
    /// `(client_id, op_id, op)` structural-op forward requests from mirror
    /// clients (a mirror workspace's split/new-tab/close/move, forwarded to run
    /// on this authoritative instance). The main loop verifies the client is the
    /// workspace holder, executes via the existing IPC handlers, and replies with
    /// a [`StreamControl::StructuralResult`](crate::stream::StreamControl).
    pub structural_ops: Vec<(StreamClientId, u64, crate::stream::StructuralOp)>,
    /// `(client_id, remote surface_id, cols, rows)` client-driven resize requests
    /// from mirror clients ([`StreamControl::ClientResize`](crate::stream::StreamControl)).
    /// The main loop verifies the client is the anchor surface's workspace holder,
    /// then resizes the real remote PTY (`Terminal::resize`) — the existing resize
    /// tap echoes the settled grid back as a `Resize` (no extra push here).
    pub resize_requests: Vec<(StreamClientId, u32, usize, usize)>,
    /// `(client_id, remote surface_id)` attention 해제 edge 요청
    /// ([`StreamControl::ClientAttentionClear`](crate::stream::StreamControl)).
    /// 미러 사용자가 그 surface 를 확인(실-포커스 / 알림 읽음)했다는 판정을 소유
    /// 인스턴스로 옮긴 것이다. 메인 루프가 anchor surface 워크스페이스의 holder 임을
    /// 검증한 뒤 서버 레코드를 지운다 — 결과는 기존 attention diff push 가
    /// `kind: null` 로 미러에 되돌려 확정한다(추가 push 없음).
    pub attention_clear_requests: Vec<(StreamClientId, u32)>,
    /// `(client_id, surface_id, width_px, height_px, pixels_per_point, theme, focused)`
    /// mesh-mirror subscribe/geometry-update requests from an attach client
    /// ([`StreamControl::MeshContext`](crate::stream::StreamControl)). The
    /// subscribe request itself doubles as capability negotiation — no
    /// separate handshake. The main loop validates holder authority +
    /// mesh whitelist membership before applying to `CoreState::mesh_mirror`.
    #[allow(clippy::type_complexity)]
    pub mesh_context_requests: Vec<(
        StreamClientId,
        u32,
        u32,
        u32,
        f32,
        Option<tasty_plugin_protocol::protocol::ThemeWire>,
        bool,
    )>,
    /// `(client_id, surface_id)` explicit full-texture-resend requests
    /// ([`StreamControl::MeshFullResendRequest`](crate::stream::StreamControl)),
    /// e.g. after a client-side decode error or reconnect.
    pub mesh_full_resend_requests: Vec<(StreamClientId, u32)>,
    /// `(client_id, surface_id, input)` — local input captured over an attach
    /// client's mesh mirror pane, forwarded verbatim
    /// ([`StreamControl::MeshInput`](crate::stream::StreamControl)).
    /// The main loop validates holder authority before appending to
    /// `CoreState::mesh_mirror`'s pending-input queue.
    pub mesh_input_events: Vec<(
        StreamClientId,
        u32,
        tasty_plugin_protocol::protocol::RawInputWire,
    )>,
    /// `(client_id, msg)` — screenshot→remote-clipboard upload chunks/commit
    /// from a mirror client. Deliberately **not** a [`StreamControl`](crate::stream::StreamControl)
    /// variant (that enum is a concurrent workstream's file) — it rides the same
    /// `StreamTag::Control` channel as a raw JSON payload with an "event" tag value
    /// `StreamControl`'s tagged parse doesn't recognize, so it falls through to the
    /// `Err(_)` arm below rather than colliding with a real `StreamControl` message.
    pub capture_uploads: Vec<(StreamClientId, CaptureUploadMsg)>,
    /// `(client_id, msg)` — file picker directory-listing requests from a
    /// mirror client (mirror asking the remote/holder side to list a directory
    /// over the same attach channel, capture-upload pattern). Same "not a
    /// `StreamControl` variant" rationale as `capture_uploads` above — rides the
    /// `StreamTag::Control` channel as a raw JSON "event"-tagged payload, tried
    /// after `CaptureUploadMsg` fails to parse.
    pub list_dir_requests: Vec<(StreamClientId, ListDirRequestMsg)>,
    /// `(client_id, msg)` — 원격 attach mirror 세션의 git 조회 요청(status/log/
    /// worktrees snapshot 또는 단일 파일 diff). `list_dir_requests` 와 동일한 이유로
    /// `StreamControl` 밖의 raw JSON "event" 태그로 온다.
    pub git_query_requests: Vec<(StreamClientId, GitQueryRequestMsg)>,
    /// `(client_id, msg)` — 원격 attach mirror 세션의 markdown 원문 조회 요청
    /// (ADR-0255). `list_dir_requests`/`git_query_requests` 와 동일한 이유로
    /// `StreamControl` 밖의 raw JSON "event" 태그로 온다.
    pub markdown_content_requests: Vec<(StreamClientId, MarkdownContentRequestMsg)>,
    /// `(client_id, event)` — native bulk 파일 전송(ADR-0054)의 begin/chunk/commit 을
    /// **도착 순서 그대로** 담는 단일 벡터. begin(Control)·chunk(Data)·commit(Control)이
    /// 서로 다른 프레임 태그로 오지만 같은 배치에 섞여 drain 될 수 있으므로, 분리된
    /// 두 벡터로 담으면 라우팅이 chunk 를 begin 보다 먼저 처리해(별도 pass) 미등록
    /// transfer 에 청크를 흘려 **전량 폐기 + 빈 파일 성공 오보**가 난다. 그래서 스크린샷
    /// capture(`CaptureChunk`/`CaptureCommit` 단일 벡터)와 동형으로 순서를 보존한다 —
    /// 라우팅은 이 벡터를 순서대로 match 해 등록/누적/확정한다. 결속 workspace 는 이
    /// 이벤트가 아니라 연결-단위 bulk 결속([`StreamHub::bulk_workspace`])에서 조회.
    pub bulk_events: Vec<(StreamClientId, BulkEvent)>,
}

/// native bulk 파일 전송의 client→server 이벤트를 **도착 순서 그대로** 담기 위한
/// 통합 enum. begin/commit 은 wire 상 [`StreamControl::BulkBegin`](crate::stream::StreamControl)
/// / [`StreamControl::BulkCommit`](crate::stream::StreamControl) (Control 프레임),
/// chunk 는 [`decode_bulk_chunk`](crate::stream::decode_bulk_chunk)로 뜯은 Data
/// 프레임이지만, `PumpOutcome` 는 이 셋을 한 벡터에 순서보존해 라우팅이 begin→chunk→
/// commit 을 올바른 순서로 처리하게 한다(capture 의 `CaptureUploadMsg` 와 동형).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkEvent {
    /// 전송 시작 — 파일명·총 크기 통지. `total_size` 는 수신측 사전 용량 승인(`begin_bulk_transfer`)의 입력.
    Begin {
        transfer_id: u64,
        filename: String,
        total_size: u64,
    },
    /// 파일 청크 — bulk 연결 Data 프레임에서 뜯은 raw 바이트. `seq` 는 진단용(TCP 는
    /// 연결당 순서 보장이라 재정렬에 쓰지 않는다).
    Chunk {
        transfer_id: u64,
        seq: u32,
        bytes: Vec<u8>,
    },
    /// 전송 완료 — 서버가 저장 확정 후 `BulkResult` 회신.
    Commit { transfer_id: u64 },
}

/// Screenshot→remote-clipboard mid-session control messages. See
/// [`PumpOutcome::capture_uploads`] doc for why this lives outside `StreamControl`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CaptureUploadMsg {
    /// One chunk of base64-encoded file bytes. `seq`/`total` are carried for
    /// diagnostics only — chunks are appended in arrival order (TCP is ordered
    /// per-connection), not reordered by `seq`.
    CaptureChunk {
        upload_id: u64,
        // 이유: 문서에 명시된 대로 진단용으로만 wire 에 실리고 읽히지 않음 —
        // engine.rs → core/ 재배치로 crate 전역 reachability 가 좁아지며 드러남.
        #[allow(dead_code)]
        seq: u32,
        #[allow(dead_code)]
        total: u32,
        data_b64: String,
    },
    /// Marks the upload complete — the main loop finalizes (write file + set the
    /// local clipboard to its path) and replies with a `capture_result` event.
    CaptureCommit { upload_id: u64, file_name: String },
}

/// File picker mid-session control messages — mirror client asking the
/// remote/holder side to list a directory. See [`PumpOutcome::list_dir_requests`]
/// doc for why this lives outside `StreamControl`. Trust model matches the screenshot
/// capture-upload channel: "attach occupancy = trust", no separate `FsRead`-style
/// permission gate (a local plugin IPC method's gate does not apply here
/// — see ADR-0042/0046).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ListDirRequestMsg {
    /// `dir` empty means "use the remote home directory" (server-side convention).
    ListDirRequest { request_id: u64, dir: String },
}

/// git-viewer(원격) mid-session control messages — mirror client asking the
/// remote/holder side for git status/log/worktrees or a single-file diff. Same
/// "outside `StreamControl`" rationale and trust model as [`ListDirRequestMsg`]
/// (attach occupancy = trust, `client_holds_workspace`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum GitQueryRequestMsg {
    GitQueryRequest {
        request_id: u64,
        /// **원격** surface id — 서버가 자기 실제 PTY(`Terminal::get_cwd`)로 cwd 를
        /// 직접 resolve 한다(mirror 의 OSC 7 재생 의존 없음). `worktree_path` 가
        /// 있으면 이 필드는 무시된다.
        surface_id: u32,
        kind: GitQueryKind,
        /// worktree 전환/새로고침 — 이전 응답이 돌려준 opaque 서버 경로를 그대로
        /// echo. `None` 이면 `surface_id` 의 cwd 로 새로 discover.
        #[serde(default)]
        worktree_path: Option<String>,
        /// `kind = Diff` 전용 — 대상 파일의 repo-relative 경로.
        #[serde(default)]
        diff_path: Option<String>,
    },
}

/// markdown mirror(ADR-0255) mid-session control messages — mirror client 가
/// 원격/holder 쪽에 그 markdown surface 가 열고 있는 문서의 **원문**을 요청한다.
/// [`ListDirRequestMsg`] 과 같은 "outside `StreamControl`" 근거와 신뢰 모델
/// (attach 점유 = 신뢰, `client_holds_workspace`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MarkdownContentRequestMsg {
    MarkdownContentRequest {
        request_id: u64,
        /// **원격** surface id — 서버가 자기 트리에서 그 surface 를 찾아 content
        /// mirror 대상인지 확인하고 그 파일을 읽는다.
        surface_id: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitQueryKind {
    Snapshot,
    Diff,
}

impl GitQueryKind {
    /// wire `kind` 필드 문자열. 서버 회신 조립(`attach_runtime`)과 클라이언트
    /// 요청 조립(`attach_client`) 양쪽이 공유해 두 곳의 문자열이 drift 하지 않게 한다.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            GitQueryKind::Snapshot => "snapshot",
            GitQueryKind::Diff => "diff",
        }
    }
}

/// Per-client push sink held in the registry.
struct StreamSink {
    tx: SyncSender<StreamFrame>,
    /// Consecutive dropped-frame count (reset on a successful send).
    lag: u32,
    /// 이 연결이 [`StreamControl::Loss`](crate::stream::StreamControl) 를 받겠다고
    /// 선언했는가. 선언은 client 가 보내는
    /// [`ClientLossNotify`](crate::stream::StreamControl::ClientLossNotify) 프레임
    /// 하나뿐이고, 선언하지 않은 연결은 종전과 **완전히 같게** 동작한다(조용한 drop).
    loss_notify: bool,
    /// 이 연결에 대해 **마지막 통지 이후** 버린 프레임 수. 통지를 실제로 태운 순간에만
    /// 0 으로 돌아간다 — 선언하지 않은 연결에서도 센다(나중에 선언해도 그때까지의 공백을
    /// 한 번에 말할 수 있어야 한다).
    pending_loss: u64,
}

/// Outcome of a [`StreamHub::push`] attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum PushResult {
    /// Frame queued for the client's write thread.
    Sent,
    /// No such client (already disconnected).
    Unknown,
    /// Sink full — frame dropped, client still connected.
    Dropped,
    /// Client exceeded the lag limit and was disconnected.
    Disconnected,
}

/// 이 허브가 **지금까지 잃은 것**. [`StreamSink::lag`] 과 다른 물음에 답한다 —
/// `lag` 은 성공 한 번에 0 으로 돌아가는 *연속* drop 수라, 단발 손실이 섞여 있어도
/// 끝에서 보면 0 이다. 여기 두 수는 **안 내려간다.**
///
/// 두 칸을 나눠 둔 이유는 손실의 성질이 다르기 때문이다. `frames` 는 연결이 **살아
/// 있는데** 사라진 프레임이라 소비자가 알 길이 없고, `lagged_out` 은 연결이 끊겨
/// 소비자가 이미 아는 손실이다. 한 수로 합치면 "조용한 손실이 있었나" 를 못 묻는다.
#[derive(Debug, Default)]
struct StreamLoss {
    frames: AtomicU64,
    lagged_out: AtomicU64,
}

/// [`StreamHub::loss`] 가 돌려주는 한 시점의 값.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamLossSnapshot {
    /// 연결이 살아 있는데 sink 가 차서 버린 프레임 수 — **조용한 손실**.
    pub frames_dropped: u64,
    /// lag 한도를 넘겨 끊은 연결 수. 그 마지막 프레임도 `frames_dropped` 에 든다.
    pub clients_lagged_out: u64,
}

/// Context handed to each accepted connection so it can register a stream sink,
/// forward inbound frames, and wake the main loop. Cloneable + `Send`.
#[derive(Clone)]
pub struct StreamContext {
    pub hub: StreamHub,
    pub inbound_tx: Sender<StreamInbound>,
    pub waker: IpcWaker,
}

/// Shared registry of stream-client push sinks. Cloneable (internal `Arc`): the
/// IPC accept threads register/unregister, the main loop pushes.
#[derive(Clone)]
pub struct StreamHub {
    sinks: Arc<Mutex<HashMap<StreamClientId, StreamSink>>>,
    next_id: Arc<AtomicU32>,
    /// bulk 파일 전송 전용 연결(ADR-0054): `client_id → 결속 workspace_id`. 이 맵에
    /// 든 연결의 `Data` 프레임은 PTY 입력이 아니라 파일 청크로 분류되고(연결-단위
    /// 태깅 — [`pump_inbound`](Self::pump_inbound)), begin/commit 인가 시 서버가 그
    /// workspace 의 holder 존재를 검증하는 결속 근거가 된다(조사 §6). 핸드셰이크에서
    /// [`register_bulk`](Self::register_bulk)로 등록, [`unregister`](Self::unregister)
    /// 로 정리.
    bulk_bindings: Arc<Mutex<HashMap<StreamClientId, u32>>>,
    /// 누적 손실. 클론이 공유한다 — 허브는 accept 스레드마다 클론되므로 `Arc` 밖에
    /// 두면 스레드마다 다른 수를 센다.
    loss: Arc<StreamLoss>,
}

impl Default for StreamHub {
    fn default() -> Self {
        Self::new()
    }
}

/// sink 맵 · bulk 결속 맵의 poison 을 각각 첫 1 회만 보고한다.
///
/// 둘 다 임계구역이 `HashMap` 조작뿐이라 패닉이 나도 불변식이 성립한다 — 복구가 맞다.
/// 반대로 `push` 는 메인 루프의 pump 경로에서 도는지라 패닉하면 창 전체가 죽는다.
/// 조용히 버리면 등록되지 않은 클라이언트가 프레임을 영영 못 받고(에러도 없다),
/// `push` 는 살아 있는 연결을 `Unknown` 으로 접는다. 근거
/// `docs/dev-guide/error-handling.md` "락 poison".
static SINKS_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static BULK_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const SINKS_WHAT: &str = "stream hub sink map";
const BULK_WHAT: &str = "stream hub bulk binding map";

impl StreamHub {
    pub fn new() -> Self {
        Self {
            sinks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU32::new(1)),
            bulk_bindings: Arc::new(Mutex::new(HashMap::new())),
            loss: Arc::new(StreamLoss::default()),
        }
    }

    /// Allocate a fresh client id (monotonic, mirrors `IdGenerator`).
    pub fn alloc_id(&self) -> StreamClientId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a client's push sink. Returns the receiving end the connection's
    /// write thread drains to the socket.
    pub fn register(&self, id: StreamClientId) -> Receiver<StreamFrame> {
        let (tx, rx) = mpsc::sync_channel(SINK_CAP);
        tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED).insert(
            id,
            StreamSink {
                tx,
                lag: 0,
                loss_notify: false,
                pending_loss: 0,
            },
        );
        rx
    }

    /// 이 연결이 손실 통지를 받겠다고 선언했음을 기록한다
    /// ([`StreamControl::ClientLossNotify`](crate::stream::StreamControl::ClientLossNotify)).
    ///
    /// 선언을 handshake 가 아니라 프레임으로 받는 이유는 판을 게이트로 쓸 수 없기
    /// 때문이다 — 서버는 `StreamOpenParams::proto` 를 **동등 비교**하므로 판을 올리면
    /// 기능이 좁아지는 것이 아니라 구 peer 의 attach 가 거절된다. 같은 모양을
    /// `MeshContext` 구독이 이미 쓴다("구독 요청 자체가 capability 협상").
    ///
    /// 멱등. 이미 끊긴 연결에 대해서는 아무것도 하지 않는다.
    pub fn enable_loss_notify(&self, id: StreamClientId) {
        if let Some(sink) =
            tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED)
                .get_mut(&id)
        {
            sink.loss_notify = true;
        }
    }

    /// Drop a client's sink (its write thread then exits when the sender drops).
    /// Idempotent. bulk 결속(있으면)도 함께 청소한다.
    pub fn unregister(&self, id: StreamClientId) {
        tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED)
            .remove(&id);
        tasty_utils::poison::recover_mutex(self.bulk_bindings.lock(), BULK_WHAT, &BULK_POISONED)
            .remove(&id);
    }

    /// bulk 전송 전용 연결(ADR-0054)로 태깅한다. 핸드셰이크의 `bulk_workspace` 를
    /// 결속 workspace 로 기록하며, 이 등록은 [`register`](Self::register)와 read 루프
    /// 시작 사이(같은 accept 스레드)에서 이뤄지므로 이후 pump 되는 모든 프레임에서
    /// [`bulk_workspace`](Self::bulk_workspace)로 조회된다.
    pub fn register_bulk(&self, id: StreamClientId, workspace_id: u32) {
        tasty_utils::poison::recover_mutex(self.bulk_bindings.lock(), BULK_WHAT, &BULK_POISONED)
            .insert(id, workspace_id);
    }

    /// 이 연결이 bulk 전용이면 결속 workspace_id, 아니면 `None`. pump_inbound 의
    /// Data 분류(파일 청크 vs PTY 입력)와 begin/commit 인가에서 참조한다.
    pub fn bulk_workspace(&self, id: StreamClientId) -> Option<u32> {
        tasty_utils::poison::recover_mutex(self.bulk_bindings.lock(), BULK_WHAT, &BULK_POISONED)
            .get(&id)
            .copied()
    }

    /// Push a frame to one client. Non-blocking: a full sink drops the frame and,
    /// past [`LAG_LIMIT`] consecutive drops, disconnects the client.
    pub fn push(&self, id: StreamClientId, frame: StreamFrame) -> PushResult {
        let mut sinks =
            tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED);
        let Some(sink) = sinks.get_mut(&id) else {
            return PushResult::Unknown;
        };
        // 손실은 **큐가 찼을 때** 난다. 그래서 통지를 그 순간에 같은 큐로 보내면 통지도
        // 함께 사라진다 — 손실을 알리는 프레임이 손실의 첫 희생자가 된다. 대신 빚으로
        // 적어 두고(`pending_loss`), 큐에 자리가 생긴 **다음 push 의 맨 앞**에서 갚는다.
        // 그 자리가 곧 스트림에서 공백이 난 지점이라, 소비자는 순번 없이도 통지 앞뒤로
        // 연속/불연속을 가를 수 있다. 자리가 아직 없으면 빚은 그대로 남고 다음 기회에
        // 다시 시도한다 — 통지는 **태워진 순간에만** 지워진다.
        Self::repay_pending_loss(sink);
        match sink.tx.try_send(frame) {
            Ok(()) => {
                sink.lag = 0;
                PushResult::Sent
            }
            Err(TrySendError::Full(_)) => {
                // 이 프레임은 어느 갈래로 가든 사라진다 — 끊는 갈래도 이것을 못 보낸다.
                // 그래서 세는 자리가 분기 **앞**이다.
                self.loss.frames.fetch_add(1, Ordering::Relaxed);
                sink.pending_loss += 1;
                sink.lag += 1;
                if sink.lag >= LAG_LIMIT {
                    sinks.remove(&id); // sender dropped → write thread exits
                    self.loss.lagged_out.fetch_add(1, Ordering::Relaxed);
                    PushResult::Disconnected
                } else {
                    PushResult::Dropped
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                sinks.remove(&id);
                PushResult::Unknown
            }
        }
    }

    /// 밀린 손실 통지를 갚는다 — [`push`](Self::push) 가 본 프레임을 태우기 **직전**에
    /// 한 번 부른다.
    ///
    /// 갚을 자리가 없으면(선언 안 함 · 빚 없음 · 큐가 아직 참) 아무것도 안 하고 빚을
    /// 그대로 둔다. 통지가 자리를 차지해 바로 뒤의 본 프레임이 떨어질 수 있는데, 그것이
    /// 의도다: 그 한 장은 다음 통지의 수에 합산되고, 반대로 통지를 뒤로 미루면 소비자가
    /// **이미 불연속인 데이터를 연속으로 그린 뒤에** 통지를 받는다.
    ///
    /// `lag` 은 건드리지 않는다. 그 수는 "소비자가 따라오고 있는가" 를 재고
    /// [`LAG_LIMIT`] 강제분리의 좌변인데, 서버가 스스로 넣은 통지의 성공을 소비자의
    /// 진척으로 세면 느린 소비자가 그 한도를 영원히 피할 수 있다.
    fn repay_pending_loss(sink: &mut StreamSink) {
        if !sink.loss_notify || sink.pending_loss == 0 {
            return;
        }
        let notice = crate::stream::StreamControl::Loss {
            frames: sink.pending_loss,
        };
        let Ok(payload) = serde_json::to_vec(&notice) else {
            return; // 빚을 유지한다 — 다음 기회에 다시 만든다.
        };
        if sink
            .tx
            .try_send(StreamFrame::new(crate::stream::StreamTag::Control, payload))
            .is_ok()
        {
            sink.pending_loss = 0;
        }
    }

    /// 지금까지의 누적 손실. 호출자가 `push` 의 반환을 안 봐도 손실이 값으로 남는
    /// 유일한 자리다 — 제품 코드 37 자리 중 31 이 `let _ =` 로 버리고, 결과를 보는
    /// 여섯도 `Dropped` 를 따로 다루지 않는다(다섯은 `Unknown`/`Disconnected` 에만
    /// 반응해 계속 보내고, 하나는 `Sent` 외 전부를 한 덩어리로 로그한다).
    pub fn loss(&self) -> StreamLossSnapshot {
        StreamLossSnapshot {
            frames_dropped: self.loss.frames.load(Ordering::Relaxed),
            clients_lagged_out: self.loss.lagged_out.load(Ordering::Relaxed),
        }
    }

    /// Number of currently connected stream clients.
    pub fn client_count(&self) -> usize {
        tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED).len()
    }

    /// Drain inbound messages routed from stream clients (called by the main loop
    /// on `AppEvent::StreamReady`). Classifies them into a [`PumpOutcome`] the
    /// main loop applies to the engine — disconnects free locks, attach requests
    /// acquire + snapshot + tap, `Data` frames route to the held surface's PTY.
    ///
    /// `Data` frames from a *non-attached* client are step-1 echo clients: the
    /// main loop echoes them back (debug only) since they aren't routed by
    /// `feed_attached_input`. Classification here has no engine access, so it
    /// returns all `Data` frames as `input_frames` and lets the main loop decide.
    pub fn pump_inbound(&self, inbound_rx: &Receiver<StreamInbound>) -> PumpOutcome {
        let mut out = PumpOutcome::default();
        while let Ok(msg) = inbound_rx.try_recv() {
            match msg {
                StreamInbound::Disconnected { client_id } => out.disconnected.push(client_id),
                StreamInbound::AttachRequest {
                    client_id,
                    target_surface_id,
                } => out.attach_requests.push((client_id, target_surface_id)),
                StreamInbound::AttachWorkspaceRequest {
                    client_id,
                    target_workspace_id,
                } => out
                    .workspace_attach_requests
                    .push((client_id, target_workspace_id)),
                StreamInbound::Frame { client_id, frame } => {
                    // 연결-단위 bulk 태깅: bulk 전용 연결이면 그 Data 는 PTY 입력이
                    // 아니라 파일 청크다(같은 `StreamTag::Data` 를 두 의미로 쓰므로
                    // 연결 단위로 구분해야 한다 — ADR-0054, 전용 연결이 필수인 이유).
                    let bulk_ws = self.bulk_workspace(client_id);
                    match frame.tag {
                        crate::stream::StreamTag::Data if bulk_ws.is_some() => {
                            match crate::stream::decode_bulk_chunk(&frame.payload) {
                                Some((transfer_id, seq, data)) => {
                                    out.bulk_events.push((
                                        client_id,
                                        BulkEvent::Chunk {
                                            transfer_id,
                                            seq,
                                            bytes: data.to_vec(),
                                        },
                                    ));
                                }
                                None => tracing::warn!(
                                    "bulk transfer: truncated chunk frame from client {client_id} (< sub-header) — dropping"
                                ),
                            }
                        }
                        crate::stream::StreamTag::Data => {
                            out.input_frames.push((client_id, frame.payload));
                        }
                        crate::stream::StreamTag::Control => {
                            // Client→server Control messages: `StructuralOp`
                            // (split/new-tab/close/move forward), `ClientResize`
                            // (client-driven mirror geometry), and the native
                            // bulk transfer control-plane (`BulkBegin`/`BulkCommit`).
                            // Any other `StreamControl` variant (server→client only)
                            // is ignored; a payload that isn't a `StreamControl` at
                            // all falls to `Err` and is tried against the screenshot
                            // capture-upload mini-protocol before being dropped.
                            match serde_json::from_slice(&frame.payload) {
                                Ok(crate::stream::StreamControl::StructuralOp { op_id, op }) => {
                                    out.structural_ops.push((client_id, op_id, op))
                                }
                                Ok(crate::stream::StreamControl::ClientResize {
                                    surface_id,
                                    cols,
                                    rows,
                                }) => {
                                    out.resize_requests
                                        .push((client_id, surface_id, cols, rows));
                                }
                                Ok(crate::stream::StreamControl::ClientAttentionClear {
                                    surface_id,
                                }) => {
                                    out.attention_clear_requests.push((client_id, surface_id));
                                }
                                Ok(crate::stream::StreamControl::BulkBegin {
                                    transfer_id,
                                    filename,
                                    total_size,
                                }) => out.bulk_events.push((
                                    client_id,
                                    BulkEvent::Begin {
                                        transfer_id,
                                        filename,
                                        total_size,
                                    },
                                )),
                                Ok(crate::stream::StreamControl::BulkCommit { transfer_id }) => out
                                    .bulk_events
                                    .push((client_id, BulkEvent::Commit { transfer_id })),
                                Ok(crate::stream::StreamControl::MeshContext {
                                    surface_id,
                                    width_px,
                                    height_px,
                                    pixels_per_point,
                                    theme,
                                    focused,
                                }) => {
                                    out.mesh_context_requests.push((
                                        client_id,
                                        surface_id,
                                        width_px,
                                        height_px,
                                        pixels_per_point,
                                        theme,
                                        focused,
                                    ));
                                }
                                Ok(crate::stream::StreamControl::MeshFullResendRequest {
                                    surface_id,
                                }) => {
                                    out.mesh_full_resend_requests.push((client_id, surface_id));
                                }
                                Ok(crate::stream::StreamControl::MeshInput {
                                    surface_id,
                                    input,
                                }) => {
                                    out.mesh_input_events.push((client_id, surface_id, input));
                                }
                                // 손실 통지 선언. 메인 루프를 거치지 않고 여기서 바로
                                // 허브 상태에 적는다 — 엔진을 한 줄도 안 보는 연결 단위
                                // 사실이고, 선언과 그 다음 `push` 사이에 메인 루프 tick 을
                                // 끼우면 그 사이의 공백을 놓친다.
                                Ok(crate::stream::StreamControl::ClientLossNotify {}) => {
                                    self.enable_loss_notify(client_id);
                                }
                                Ok(_) => {}
                                Err(_) => {
                                    if let Ok(msg) =
                                        serde_json::from_slice::<CaptureUploadMsg>(&frame.payload)
                                    {
                                        out.capture_uploads.push((client_id, msg));
                                    } else if let Ok(msg) =
                                        serde_json::from_slice::<ListDirRequestMsg>(&frame.payload)
                                    {
                                        out.list_dir_requests.push((client_id, msg));
                                    } else if let Ok(msg) =
                                        serde_json::from_slice::<GitQueryRequestMsg>(&frame.payload)
                                    {
                                        out.git_query_requests.push((client_id, msg));
                                    } else if let Ok(msg) =
                                        serde_json::from_slice::<MarkdownContentRequestMsg>(
                                            &frame.payload,
                                        )
                                    {
                                        out.markdown_content_requests.push((client_id, msg));
                                    }
                                }
                            }
                        }
                        // Ping/Detach carry no step-4 payload. Ping's only job is
                        // completing the accept thread's `read_frame` call so the
                        // socket's read timeout resets (heartbeat protocol) — no
                        // state to track here.
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamTag;

    fn frame(tag: StreamTag, p: &[u8]) -> StreamFrame {
        StreamFrame::new(tag, p.to_vec())
    }

    #[test]
    fn alloc_id_is_monotonic() {
        let hub = StreamHub::new();
        assert_eq!(hub.alloc_id(), 1);
        assert_eq!(hub.alloc_id(), 2);
        assert_eq!(hub.alloc_id(), 3);
    }

    #[test]
    fn push_unknown_client() {
        let hub = StreamHub::new();
        assert_eq!(
            hub.push(42, frame(StreamTag::Data, b"x")),
            PushResult::Unknown
        );
    }

    #[test]
    fn register_push_receive() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        assert_eq!(hub.client_count(), 1);
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"hi")),
            PushResult::Sent
        );
        let got = rx.recv().unwrap();
        assert_eq!(got.tag, StreamTag::Data);
        assert_eq!(got.payload, b"hi");
        hub.unregister(id);
        assert_eq!(hub.client_count(), 0);
    }

    #[test]
    fn slow_client_drops_then_disconnects() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        // Keep the receiver alive but never drain it so the sink fills up.
        let _rx = hub.register(id);
        // Fill the bounded sink (SINK_CAP frames accepted).
        for _ in 0..SINK_CAP {
            assert_eq!(hub.push(id, frame(StreamTag::Data, b"x")), PushResult::Sent);
        }
        // Next pushes are dropped until LAG_LIMIT, then the client is dropped.
        let mut saw_disconnect = false;
        for _ in 0..LAG_LIMIT {
            match hub.push(id, frame(StreamTag::Data, b"x")) {
                PushResult::Dropped => {}
                PushResult::Disconnected => {
                    saw_disconnect = true;
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(saw_disconnect);
        assert_eq!(hub.client_count(), 0);
    }

    /// 누적 손실은 **안 내려간다** — `StreamSink::lag` 이 성공 한 번에 0 이 되는 것과
    /// 다른 물음에 답한다. 단발 drop 뒤에 성공이 오면 `lag` 으로는 아무 일도 없던 것처럼
    /// 보이고, 그것이 이 카운터가 있는 이유다.
    #[test]
    fn a_single_drop_survives_the_success_that_follows_it() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        assert_eq!(
            hub.loss(),
            StreamLossSnapshot::default(),
            "시작이 0 이 아니다"
        );

        // sink 를 채운다 — 여기까지는 손실이 없다.
        for _ in 0..SINK_CAP {
            assert_eq!(hub.push(id, frame(StreamTag::Data, b"x")), PushResult::Sent);
        }
        assert_eq!(hub.loss().frames_dropped, 0, "성공만 했는데 손실을 셌다");

        // 한 장 잃는다.
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"lost")),
            PushResult::Dropped
        );
        assert_eq!(hub.loss().frames_dropped, 1);
        assert_eq!(hub.loss().clients_lagged_out, 0, "끊지도 않았는데 셌다");

        // 소비자가 한 칸을 비우면 다음 push 는 성공하고 `lag` 은 0 으로 돌아간다.
        rx.recv().expect("한 장은 이미 큐에 있다");
        assert_eq!(hub.push(id, frame(StreamTag::Data, b"y")), PushResult::Sent);

        // 그래도 잃은 한 장은 남아 있다 — 이것이 `lag` 과 갈리는 자리다.
        assert_eq!(
            hub.loss().frames_dropped,
            1,
            "성공이 누적 손실을 지웠다 — lag 을 그대로 노출한 것과 같다"
        );
    }

    /// 끊는 갈래도 그 프레임을 못 보낸다. 그래서 `frames_dropped` 는 끊김 직전의
    /// 마지막 한 장까지 세고, 끊긴 연결 수는 **따로** 센다 — 한 수로 합치면
    /// "조용한 손실이 있었나" 를 못 묻는다.
    #[test]
    fn the_frame_that_triggers_the_disconnect_is_counted_as_lost_too() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let _rx = hub.register(id);
        for _ in 0..SINK_CAP {
            assert_eq!(hub.push(id, frame(StreamTag::Data, b"x")), PushResult::Sent);
        }
        let mut refused = 0;
        loop {
            match hub.push(id, frame(StreamTag::Data, b"x")) {
                PushResult::Dropped => refused += 1,
                PushResult::Disconnected => {
                    refused += 1;
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        let loss = hub.loss();
        assert_eq!(loss.frames_dropped, refused, "끊은 갈래의 한 장을 안 셌다");
        assert_eq!(loss.clients_lagged_out, 1);
    }

    /// 손실 통지를 선언하지 **않은** 연결은 종전과 완전히 같다 — 큐에 들어오는 것이
    /// 프레임 하나도 안 늘어난다. 이 시험이 지키는 것은 "기존 peer 무영향" 이다.
    #[test]
    fn a_client_that_never_declares_sees_no_extra_frame() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        for _ in 0..SINK_CAP {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"pre")),
                PushResult::Sent
            );
        }
        for _ in 0..3 {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"gap")),
                PushResult::Dropped
            );
        }
        rx.recv().expect("한 장 비운다");
        rx.recv().expect("두 장 비운다");
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"post")),
            PushResult::Sent
        );

        let drained: Vec<StreamFrame> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            drained.iter().all(|f| f.tag == StreamTag::Data),
            "선언하지 않았는데 Control 프레임이 섞였다"
        );
        assert_eq!(drained.len(), SINK_CAP - 1, "큐 길이가 달라졌다");
    }

    /// 통지의 값은 **수만이 아니라 자리**다. 공백이 난 지점, 즉 공백을 넘어 살아남은
    /// 첫 프레임 **바로 앞**에 들어가야 소비자가 순번 없이 앞뒤를 가를 수 있다.
    #[test]
    fn the_notice_lands_between_the_last_survivor_and_the_first_frame_after_the_gap() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        hub.enable_loss_notify(id);

        for _ in 0..SINK_CAP {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"pre")),
                PushResult::Sent
            );
        }
        for _ in 0..3 {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"gap")),
                PushResult::Dropped
            );
        }
        // 소비자가 두 칸을 비운다 — 한 칸은 통지가, 한 칸은 본 프레임이 쓴다.
        rx.recv().expect("한 장 비운다");
        rx.recv().expect("두 장 비운다");
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"post")),
            PushResult::Sent
        );

        let drained: Vec<StreamFrame> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let notice_at = drained
            .iter()
            .position(|f| f.tag == StreamTag::Control)
            .expect("통지가 큐에 없다");
        assert_eq!(
            notice_at,
            drained.len() - 2,
            "통지가 공백 지점이 아니라 다른 자리에 있다"
        );
        assert_eq!(drained[notice_at + 1].payload, b"post".to_vec());
        assert!(
            drained[..notice_at].iter().all(|f| f.payload == b"pre"),
            "통지 앞에 공백 뒤 프레임이 섞였다"
        );
        let parsed: crate::stream::StreamControl =
            serde_json::from_slice(&drained[notice_at].payload).expect("통지 파싱");
        assert_eq!(
            parsed,
            crate::stream::StreamControl::Loss { frames: 3 },
            "잃은 수가 안 맞는다"
        );
    }

    /// 통지 자체가 막히는 갈래 — 큐가 아직 차 있으면 통지는 **안 태워지고 빚으로 남는다.**
    /// 그 사이에 더 잃으면 빚이 커지고, 결국 한 번에 갚는다. 빚을 태우기도 전에 지우면
    /// 그 공백은 영영 안 알려진다.
    #[test]
    fn a_notice_that_cannot_be_sent_is_kept_as_debt_and_paid_in_full_later() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        hub.enable_loss_notify(id);

        for _ in 0..SINK_CAP {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"pre")),
                PushResult::Sent
            );
        }
        // 자리가 없는 동안의 두 번 — 통지도 본 프레임도 못 들어간다.
        for _ in 0..2 {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"gap")),
                PushResult::Dropped
            );
        }
        // 한 칸만 비우면 그 칸은 통지가 쓴다(본 프레임은 또 떨어진다).
        rx.recv().expect("한 장 비운다");
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"gap")),
            PushResult::Dropped
        );

        rx.recv().expect("소비자가 계속 읽는다");
        let drained: Vec<StreamFrame> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let notice = drained
            .iter()
            .find(|f| f.tag == StreamTag::Control)
            .expect("통지가 결국에도 안 왔다");
        let parsed: crate::stream::StreamControl =
            serde_json::from_slice(&notice.payload).expect("통지 파싱");
        assert_eq!(
            parsed,
            crate::stream::StreamControl::Loss { frames: 2 },
            "통지가 태워지기 전에 빚이 지워졌다"
        );
    }

    /// 통지를 태운 것은 **소비자의 진척이 아니다.** `lag` 을 같이 0 으로 돌리면 느린
    /// 소비자가 [`LAG_LIMIT`] 강제분리를 영원히 피한다. 좌변: 한 칸이 비어 통지가
    /// 들어간 바로 그 push 에서 연속 drop 수가 한도에 닿아야 한다.
    #[test]
    fn sending_the_notice_does_not_count_as_the_consumer_keeping_up() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        hub.enable_loss_notify(id);

        for _ in 0..SINK_CAP {
            assert_eq!(hub.push(id, frame(StreamTag::Data, b"x")), PushResult::Sent);
        }
        for _ in 0..(LAG_LIMIT - 1) {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"x")),
                PushResult::Dropped
            );
        }
        rx.recv().expect("한 칸 비운다 — 통지가 그 칸을 쓴다");
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"x")),
            PushResult::Disconnected,
            "통지 성공이 lag 을 되돌렸다 — 느린 소비자가 한도를 피한다"
        );
    }

    /// 선언은 메인 루프를 거치지 않고 `pump_inbound` 가 바로 허브에 적는다.
    #[test]
    fn pump_inbound_records_the_loss_notify_declaration() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let rx = hub.register(id);
        let (tx, inbound_rx) = mpsc::channel();
        let payload =
            serde_json::to_vec(&crate::stream::StreamControl::ClientLossNotify {}).unwrap();
        tx.send(StreamInbound::Frame {
            client_id: id,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert!(
            out.input_frames.is_empty() && out.structural_ops.is_empty(),
            "선언이 다른 갈래로 샜다"
        );

        for _ in 0..SINK_CAP {
            assert_eq!(
                hub.push(id, frame(StreamTag::Data, b"pre")),
                PushResult::Sent
            );
        }
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"gap")),
            PushResult::Dropped
        );
        rx.recv().expect("한 장 비운다");
        rx.recv().expect("두 장 비운다");
        assert_eq!(
            hub.push(id, frame(StreamTag::Data, b"post")),
            PushResult::Sent
        );
        let drained: Vec<StreamFrame> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            drained.iter().any(|f| f.tag == StreamTag::Control),
            "pump 로 들어온 선언이 안 기록됐다"
        );
    }

    /// 허브는 accept 스레드마다 클론된다. 카운터가 `Arc` 밖에 있으면 클론마다 다른 수를
    /// 세고, 어느 수도 전체 손실이 아니게 된다.
    #[test]
    fn a_clone_of_the_hub_counts_into_the_same_total() {
        let hub = StreamHub::new();
        let id = hub.alloc_id();
        let _rx = hub.register(id);
        for _ in 0..SINK_CAP {
            assert_eq!(hub.push(id, frame(StreamTag::Data, b"x")), PushResult::Sent);
        }
        let other = hub.clone();
        assert_eq!(
            other.push(id, frame(StreamTag::Data, b"lost")),
            PushResult::Dropped
        );
        assert_eq!(hub.loss().frames_dropped, 1, "클론이 자기 수를 따로 셌다");
    }

    #[test]
    fn pump_inbound_classifies_data_as_input() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        tx.send(StreamInbound::Frame {
            client_id: 5,
            frame: frame(StreamTag::Data, b"echo me"),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert!(out.disconnected.is_empty());
        assert!(out.attach_requests.is_empty());
        assert_eq!(out.input_frames, vec![(5u32, b"echo me".to_vec())]);
    }

    #[test]
    fn pump_inbound_classifies_attach_requests() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        tx.send(StreamInbound::AttachRequest {
            client_id: 3,
            target_surface_id: 42,
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.attach_requests, vec![(3u32, 42u32)]);
    }

    #[test]
    fn pump_inbound_classifies_workspace_attach_requests() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        tx.send(StreamInbound::AttachWorkspaceRequest {
            client_id: 4,
            target_workspace_id: 8,
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.workspace_attach_requests, vec![(4u32, 8u32)]);
        assert!(out.attach_requests.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_structural_op() {
        use crate::stream::{SplitAxis, StreamControl, StructuralOp};
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let op = StructuralOp::SplitSurface {
            surface_id: 12,
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let payload = serde_json::to_vec(&StreamControl::StructuralOp {
            op_id: 3,
            op: op.clone(),
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 8,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.structural_ops, vec![(8u32, 3u64, op)]);
        // Not misclassified as input.
        assert!(out.input_frames.is_empty());
    }

    #[test]
    fn pump_inbound_ignores_unknown_control() {
        // A Control frame that is not a client→server message (e.g. a Resize,
        // which is server→client only) must not be classified as a structural op
        // or a resize request.
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let payload = serde_json::to_vec(&crate::stream::StreamControl::Resize {
            surface_id: 1,
            cols: 80,
            rows: 24,
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 8,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert!(out.structural_ops.is_empty());
        assert!(out.resize_requests.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_client_resize() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let payload = serde_json::to_vec(&crate::stream::StreamControl::ClientResize {
            surface_id: 12,
            cols: 203,
            rows: 57,
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 8,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.resize_requests, vec![(8u32, 12u32, 203usize, 57usize)]);
        // Not misclassified as a structural op or input.
        assert!(out.structural_ops.is_empty());
        assert!(out.input_frames.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_mesh_context() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let payload = serde_json::to_vec(&crate::stream::StreamControl::MeshContext {
            surface_id: 7,
            width_px: 800,
            height_px: 600,
            pixels_per_point: 2.0,
            theme: None,
            focused: true,
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 9,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(
            out.mesh_context_requests,
            vec![(9u32, 7u32, 800u32, 600u32, 2.0f32, None, true)]
        );
        assert!(out.resize_requests.is_empty());
        assert!(out.structural_ops.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_mesh_full_resend_request() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let payload = serde_json::to_vec(&crate::stream::StreamControl::MeshFullResendRequest {
            surface_id: 7,
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 9,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.mesh_full_resend_requests, vec![(9u32, 7u32)]);
        assert!(out.mesh_context_requests.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_mesh_input() {
        use tasty_plugin_protocol::protocol::{ModifiersWire, RawInputEventWire, RawInputWire};
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let input = RawInputWire {
            time: None,
            focused: true,
            modifiers: ModifiersWire {
                shift: true,
                ..Default::default()
            },
            events: vec![RawInputEventWire::PointerMoved { x: 3.0, y: 4.0 }],
        };
        let payload = serde_json::to_vec(&crate::stream::StreamControl::MeshInput {
            surface_id: 7,
            input: input.clone(),
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 9,
            frame: frame(StreamTag::Control, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.mesh_input_events, vec![(9u32, 7u32, input)]);
        assert!(out.mesh_context_requests.is_empty());
        assert!(out.mesh_full_resend_requests.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_capture_chunk_and_commit() {
        // The capture-upload mini-protocol lives outside `StreamControl` — its
        // payloads must fail the `StreamControl` parse (unrecognized "event") and
        // fall through to the `CaptureUploadMsg` attempt.
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let chunk = serde_json::json!({
            "event": "capture_chunk",
            "upload_id": 42,
            "seq": 0,
            "total": 1,
            "data_b64": "aGVsbG8=",
        });
        let commit = serde_json::json!({
            "event": "capture_commit",
            "upload_id": 42,
            "file_name": "screenshot-1.png",
        });
        tx.send(StreamInbound::Frame {
            client_id: 5,
            frame: frame(StreamTag::Control, chunk.to_string().as_bytes()),
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 5,
            frame: frame(StreamTag::Control, commit.to_string().as_bytes()),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.capture_uploads.len(), 2);
        match &out.capture_uploads[0] {
            (5, CaptureUploadMsg::CaptureChunk { upload_id, .. }) => assert_eq!(*upload_id, 42),
            other => panic!("expected CaptureChunk, got {other:?}"),
        }
        match &out.capture_uploads[1] {
            (
                5,
                CaptureUploadMsg::CaptureCommit {
                    upload_id,
                    file_name,
                },
            ) => {
                assert_eq!(*upload_id, 42);
                assert_eq!(file_name, "screenshot-1.png");
            }
            other => panic!("expected CaptureCommit, got {other:?}"),
        }
        // Not misclassified as a structural op / resize / input frame.
        assert!(out.structural_ops.is_empty());
        assert!(out.resize_requests.is_empty());
        assert!(out.input_frames.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_list_dir_request() {
        // File picker: same "outside StreamControl" pattern as capture upload,
        // tried only after CaptureUploadMsg fails to parse.
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let req = serde_json::json!({
            "event": "list_dir_request",
            "request_id": 7,
            "dir": "/tmp",
        });
        tx.send(StreamInbound::Frame {
            client_id: 3,
            frame: frame(StreamTag::Control, req.to_string().as_bytes()),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.list_dir_requests.len(), 1);
        match &out.list_dir_requests[0] {
            (3, ListDirRequestMsg::ListDirRequest { request_id, dir }) => {
                assert_eq!(*request_id, 7);
                assert_eq!(dir, "/tmp");
            }
            other => panic!("expected ListDirRequest from client 3, got {other:?}"),
        }
        assert!(out.capture_uploads.is_empty());
        assert!(out.structural_ops.is_empty());
        assert!(out.resize_requests.is_empty());
        assert!(out.input_frames.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_git_query_request() {
        // git-viewer(원격): list_dir_request 와 동일한 "outside StreamControl" 패턴.
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        let req = serde_json::json!({
            "event": "git_query_request",
            "request_id": 9,
            "surface_id": 42,
            "kind": "snapshot",
        });
        tx.send(StreamInbound::Frame {
            client_id: 3,
            frame: frame(StreamTag::Control, req.to_string().as_bytes()),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.git_query_requests.len(), 1);
        match &out.git_query_requests[0] {
            (
                3,
                GitQueryRequestMsg::GitQueryRequest {
                    request_id,
                    surface_id,
                    kind,
                    worktree_path,
                    diff_path,
                },
            ) => {
                assert_eq!(*request_id, 9);
                assert_eq!(*surface_id, 42);
                assert_eq!(*kind, GitQueryKind::Snapshot);
                assert_eq!(*worktree_path, None);
                assert_eq!(*diff_path, None);
            }
            other => panic!("expected GitQueryRequest from client 3, got {other:?}"),
        }
        assert!(out.list_dir_requests.is_empty());
        assert!(out.capture_uploads.is_empty());
    }

    #[test]
    fn bulk_binding_register_and_unregister() {
        let hub = StreamHub::new();
        assert_eq!(hub.bulk_workspace(5), None);
        hub.register_bulk(5, 42);
        assert_eq!(hub.bulk_workspace(5), Some(42));
        // 다른 client 는 영향 없음.
        assert_eq!(hub.bulk_workspace(6), None);
        hub.unregister(5);
        assert_eq!(hub.bulk_workspace(5), None);
    }

    #[test]
    fn pump_inbound_bulk_connection_data_is_chunk_not_input() {
        // bulk 로 태깅된 연결의 Data 는 파일 청크(bulk_events::Chunk)로 분류되고
        // input_frames(PTY)로 새지 않는다.
        let hub = StreamHub::new();
        hub.register_bulk(7, 3); // client 7 = bulk 연결(ws 3 결속)
        let (tx, inbound_rx) = mpsc::channel();
        let payload = crate::stream::encode_bulk_chunk(99, 2, b"filebytes");
        tx.send(StreamInbound::Frame {
            client_id: 7,
            frame: frame(StreamTag::Data, &payload),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(
            out.bulk_events,
            vec![(
                7u32,
                BulkEvent::Chunk {
                    transfer_id: 99,
                    seq: 2,
                    bytes: b"filebytes".to_vec(),
                }
            )]
        );
        assert!(out.input_frames.is_empty());
    }

    #[test]
    fn pump_inbound_non_bulk_data_still_input() {
        // 비-bulk 연결의 Data 는 종전대로 PTY 입력으로 간다(회귀 방지).
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        tx.send(StreamInbound::Frame {
            client_id: 8,
            frame: frame(StreamTag::Data, b"keystrokes"),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.input_frames, vec![(8u32, b"keystrokes".to_vec())]);
        assert!(out.bulk_events.is_empty());
    }

    #[test]
    fn pump_inbound_classifies_bulk_begin_and_commit() {
        use crate::stream::StreamControl;
        let hub = StreamHub::new();
        hub.register_bulk(9, 1);
        let (tx, inbound_rx) = mpsc::channel();
        let begin = serde_json::to_vec(&StreamControl::BulkBegin {
            transfer_id: 100,
            filename: "img.png".to_string(),
            total_size: 2048,
        })
        .unwrap();
        let commit = serde_json::to_vec(&StreamControl::BulkCommit { transfer_id: 100 }).unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 9,
            frame: frame(StreamTag::Control, &begin),
        })
        .unwrap();
        tx.send(StreamInbound::Frame {
            client_id: 9,
            frame: frame(StreamTag::Control, &commit),
        })
        .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.bulk_events.len(), 2);
        assert_eq!(
            out.bulk_events[0],
            (
                9u32,
                BulkEvent::Begin {
                    transfer_id: 100,
                    filename: "img.png".to_string(),
                    total_size: 2048,
                }
            )
        );
        assert_eq!(
            out.bulk_events[1],
            (9u32, BulkEvent::Commit { transfer_id: 100 })
        );
        // 구조 op / capture 로 오분류되지 않음.
        assert!(out.structural_ops.is_empty());
        assert!(out.capture_uploads.is_empty());
    }

    #[test]
    fn pump_inbound_preserves_bulk_begin_chunk_commit_order() {
        // 회귀 방지(Gate4): begin+chunk*2+commit 이 **한 배치**에 함께 drain 돼도
        // bulk_events 가 도착 순서를 그대로 보존해야 한다(분리 벡터였을 때의
        // chunk-before-begin data-loss 결함 재발 방지). 라우팅이 이 순서대로 처리하면
        // begin→append→append→finalize 로 전량 저장된다.
        use crate::stream::{StreamControl, encode_bulk_chunk};
        let hub = StreamHub::new();
        hub.register_bulk(5, 2);
        let (tx, inbound_rx) = mpsc::channel();
        let begin = serde_json::to_vec(&StreamControl::BulkBegin {
            transfer_id: 7,
            filename: "f.bin".to_string(),
            total_size: 6,
        })
        .unwrap();
        let commit = serde_json::to_vec(&StreamControl::BulkCommit { transfer_id: 7 }).unwrap();
        for f in [
            frame(StreamTag::Control, &begin),
            frame(StreamTag::Data, &encode_bulk_chunk(7, 0, b"abc")),
            frame(StreamTag::Data, &encode_bulk_chunk(7, 1, b"def")),
            frame(StreamTag::Control, &commit),
        ] {
            tx.send(StreamInbound::Frame {
                client_id: 5,
                frame: f,
            })
            .unwrap();
        }
        let out = hub.pump_inbound(&inbound_rx);
        let events: Vec<&BulkEvent> = out.bulk_events.iter().map(|(_, e)| e).collect();
        assert!(matches!(events[0], BulkEvent::Begin { transfer_id: 7, .. }));
        assert!(matches!(
            events[1],
            BulkEvent::Chunk {
                transfer_id: 7,
                seq: 0,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            BulkEvent::Chunk {
                transfer_id: 7,
                seq: 1,
                ..
            }
        ));
        assert!(matches!(events[3], BulkEvent::Commit { transfer_id: 7 }));
    }

    // begin/chunk/commit 이 한 배치로 와도 저장 bytes 가 온전한가를 `BulkTransferRegistry`
    // 까지 이어 재는 end-to-end 시험은 본체 `src/core/bulk_transfer.rs` 에 있다 — 이
    // 크레이트는 본체 core 를 참조할 수 없다. 순서 보존 자체는 바로 위 시험이 여기서 잰다.

    #[test]
    fn pump_inbound_reports_disconnects() {
        let hub = StreamHub::new();
        let (tx, inbound_rx) = mpsc::channel();
        tx.send(StreamInbound::Disconnected { client_id: 7 })
            .unwrap();
        tx.send(StreamInbound::Disconnected { client_id: 9 })
            .unwrap();
        let out = hub.pump_inbound(&inbound_rx);
        assert_eq!(out.disconnected, vec![7, 9]);
    }
}
