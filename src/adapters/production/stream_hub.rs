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
//! See `.claude-workspace/conductor/attach-detach/step1/plan.md` §5, §7.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use crate::ipc::server::IpcWaker;
use crate::ipc::stream::StreamFrame;

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
    /// a [`StreamControl::StructuralResult`](crate::ipc::stream::StreamControl).
    pub structural_ops: Vec<(StreamClientId, u64, crate::ipc::stream::StructuralOp)>,
    /// `(client_id, remote surface_id, cols, rows)` client-driven resize requests
    /// from mirror clients ([`StreamControl::ClientResize`](crate::ipc::stream::StreamControl)).
    /// The main loop verifies the client is the anchor surface's workspace holder,
    /// then resizes the real remote PTY (`Terminal::resize`) — the existing resize
    /// tap echoes the settled grid back as a `Resize` (no extra push here).
    pub resize_requests: Vec<(StreamClientId, u32, usize, usize)>,
    /// `(client_id, msg)` — (03) screenshot→remote-clipboard upload chunks/commit
    /// from a mirror client. Deliberately **not** a [`StreamControl`](crate::ipc::stream::StreamControl)
    /// variant (that enum is a concurrent workstream's file) — it rides the same
    /// `StreamTag::Control` channel as a raw JSON payload with an "event" tag value
    /// `StreamControl`'s tagged parse doesn't recognize, so it falls through to the
    /// `Err(_)` arm below rather than colliding with a real `StreamControl` message.
    pub capture_uploads: Vec<(StreamClientId, CaptureUploadMsg)>,
    /// `(client_id, msg)` — (04) file picker directory-listing requests from a
    /// mirror client (mirror asking the remote/holder side to list a directory
    /// over the same attach channel, capture-upload pattern). Same "not a
    /// `StreamControl` variant" rationale as `capture_uploads` above — rides the
    /// `StreamTag::Control` channel as a raw JSON "event"-tagged payload, tried
    /// after `CaptureUploadMsg` fails to parse.
    pub list_dir_requests: Vec<(StreamClientId, ListDirRequestMsg)>,
    /// `(client_id, event)` — (06) native bulk 파일 전송의 begin/chunk/commit 을
    /// **도착 순서 그대로** 담는 단일 벡터. begin(Control)·chunk(Data)·commit(Control)이
    /// 서로 다른 프레임 태그로 오지만 같은 배치에 섞여 drain 될 수 있으므로, 분리된
    /// 두 벡터로 담으면 라우팅이 chunk 를 begin 보다 먼저 처리해(별도 pass) 미등록
    /// transfer 에 청크를 흘려 **전량 폐기 + 빈 파일 성공 오보**가 난다. 그래서 (03)
    /// capture(`CaptureChunk`/`CaptureCommit` 단일 벡터)와 동형으로 순서를 보존한다 —
    /// 라우팅은 이 벡터를 순서대로 match 해 등록/누적/확정한다. 결속 workspace 는 이
    /// 이벤트가 아니라 연결-단위 bulk 결속([`StreamHub::bulk_workspace`])에서 조회.
    pub bulk_events: Vec<(StreamClientId, BulkEvent)>,
}

/// (06) native bulk 파일 전송의 client→server 이벤트를 **도착 순서 그대로** 담기 위한
/// 통합 enum. begin/commit 은 wire 상 [`StreamControl::BulkBegin`](crate::ipc::stream::StreamControl)
/// / [`StreamControl::BulkCommit`](crate::ipc::stream::StreamControl) (Control 프레임),
/// chunk 는 [`decode_bulk_chunk`](crate::ipc::stream::decode_bulk_chunk)로 뜯은 Data
/// 프레임이지만, `PumpOutcome` 는 이 셋을 한 벡터에 순서보존해 라우팅이 begin→chunk→
/// commit 을 올바른 순서로 처리하게 한다(capture 의 `CaptureUploadMsg` 와 동형).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkEvent {
    /// 전송 시작 — 파일명·총 크기 통지. `total_size` 는 사전 용량 승인(07)의 입력.
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

/// (03) screenshot→remote-clipboard mid-session control messages. See
/// [`PumpOutcome::capture_uploads`] doc for why this lives outside `StreamControl`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CaptureUploadMsg {
    /// One chunk of base64-encoded file bytes. `seq`/`total` are carried for
    /// diagnostics only — chunks are appended in arrival order (TCP is ordered
    /// per-connection), not reordered by `seq`.
    CaptureChunk {
        upload_id: u64,
        seq: u32,
        total: u32,
        data_b64: String,
    },
    /// Marks the upload complete — the main loop finalizes (write file + set the
    /// local clipboard to its path) and replies with a `capture_result` event.
    CaptureCommit { upload_id: u64, file_name: String },
}

/// (04) file picker mid-session control messages — mirror client asking the
/// remote/holder side to list a directory. See [`PumpOutcome::list_dir_requests`]
/// doc for why this lives outside `StreamControl`. Trust model matches the (03)
/// capture-upload channel: "attach occupancy = trust", no separate `FsRead`-style
/// permission gate (the local `fs.pick_file` IPC method's gate does not apply here
/// — see ADR-0042/0046).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ListDirRequestMsg {
    /// `dir` empty means "use the remote home directory" (server-side convention).
    ListDirRequest { request_id: u64, dir: String },
}

/// Per-client push sink held in the registry.
struct StreamSink {
    tx: SyncSender<StreamFrame>,
    /// Consecutive dropped-frame count (reset on a successful send).
    lag: u32,
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
}

impl Default for StreamHub {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamHub {
    pub fn new() -> Self {
        Self {
            sinks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU32::new(1)),
            bulk_bindings: Arc::new(Mutex::new(HashMap::new())),
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
        if let Ok(mut sinks) = self.sinks.lock() {
            sinks.insert(id, StreamSink { tx, lag: 0 });
        }
        rx
    }

    /// Drop a client's sink (its write thread then exits when the sender drops).
    /// Idempotent. bulk 결속(있으면)도 함께 청소한다.
    pub fn unregister(&self, id: StreamClientId) {
        if let Ok(mut sinks) = self.sinks.lock() {
            sinks.remove(&id);
        }
        if let Ok(mut bulk) = self.bulk_bindings.lock() {
            bulk.remove(&id);
        }
    }

    /// bulk 전송 전용 연결(ADR-0054)로 태깅한다. 핸드셰이크의 `bulk_workspace` 를
    /// 결속 workspace 로 기록하며, 이 등록은 [`register`](Self::register)와 read 루프
    /// 시작 사이(같은 accept 스레드)에서 이뤄지므로 이후 pump 되는 모든 프레임에서
    /// [`bulk_workspace`](Self::bulk_workspace)로 조회된다.
    pub fn register_bulk(&self, id: StreamClientId, workspace_id: u32) {
        if let Ok(mut bulk) = self.bulk_bindings.lock() {
            bulk.insert(id, workspace_id);
        }
    }

    /// 이 연결이 bulk 전용이면 결속 workspace_id, 아니면 `None`. pump_inbound 의
    /// Data 분류(파일 청크 vs PTY 입력)와 begin/commit 인가에서 참조한다.
    pub fn bulk_workspace(&self, id: StreamClientId) -> Option<u32> {
        self.bulk_bindings
            .lock()
            .ok()
            .and_then(|b| b.get(&id).copied())
    }

    /// Push a frame to one client. Non-blocking: a full sink drops the frame and,
    /// past [`LAG_LIMIT`] consecutive drops, disconnects the client.
    pub fn push(&self, id: StreamClientId, frame: StreamFrame) -> PushResult {
        let Ok(mut sinks) = self.sinks.lock() else {
            return PushResult::Unknown;
        };
        let Some(sink) = sinks.get_mut(&id) else {
            return PushResult::Unknown;
        };
        match sink.tx.try_send(frame) {
            Ok(()) => {
                sink.lag = 0;
                PushResult::Sent
            }
            Err(TrySendError::Full(_)) => {
                sink.lag += 1;
                if sink.lag >= LAG_LIMIT {
                    sinks.remove(&id); // sender dropped → write thread exits
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

    /// Number of currently connected stream clients.
    pub fn client_count(&self) -> usize {
        self.sinks.lock().map(|s| s.len()).unwrap_or(0)
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
                        crate::ipc::stream::StreamTag::Data if bulk_ws.is_some() => {
                            match crate::ipc::stream::decode_bulk_chunk(&frame.payload) {
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
                        crate::ipc::stream::StreamTag::Data => {
                            out.input_frames.push((client_id, frame.payload));
                        }
                        crate::ipc::stream::StreamTag::Control => {
                            // Client→server Control messages: `StructuralOp`
                            // (split/new-tab/close/move forward), `ClientResize`
                            // (client-driven mirror geometry), and the (06) native
                            // bulk transfer control-plane (`BulkBegin`/`BulkCommit`).
                            // Any other `StreamControl` variant (server→client only)
                            // is ignored; a payload that isn't a `StreamControl` at
                            // all falls to `Err` and is tried against the (03)
                            // capture-upload mini-protocol before being dropped.
                            match serde_json::from_slice(&frame.payload) {
                                Ok(crate::ipc::stream::StreamControl::StructuralOp {
                                    op_id,
                                    op,
                                }) => out.structural_ops.push((client_id, op_id, op)),
                                Ok(crate::ipc::stream::StreamControl::ClientResize {
                                    surface_id,
                                    cols,
                                    rows,
                                }) => {
                                    out.resize_requests
                                        .push((client_id, surface_id, cols, rows));
                                }
                                Ok(crate::ipc::stream::StreamControl::BulkBegin {
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
                                Ok(crate::ipc::stream::StreamControl::BulkCommit {
                                    transfer_id,
                                }) => out
                                    .bulk_events
                                    .push((client_id, BulkEvent::Commit { transfer_id })),
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
    use crate::ipc::stream::StreamTag;

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
        use crate::ipc::stream::{SplitAxis, StreamControl, StructuralOp};
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
        let payload = serde_json::to_vec(&crate::ipc::stream::StreamControl::Resize {
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
        let payload = serde_json::to_vec(&crate::ipc::stream::StreamControl::ClientResize {
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
    fn pump_inbound_classifies_capture_chunk_and_commit() {
        // (03) The capture-upload mini-protocol lives outside `StreamControl` — its
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
        // (04) file picker: same "outside StreamControl" pattern as capture upload,
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
        // (06) bulk 로 태깅된 연결의 Data 는 파일 청크(bulk_events::Chunk)로 분류되고
        // input_frames(PTY)로 새지 않는다.
        let hub = StreamHub::new();
        hub.register_bulk(7, 3); // client 7 = bulk 연결(ws 3 결속)
        let (tx, inbound_rx) = mpsc::channel();
        let payload = crate::ipc::stream::encode_bulk_chunk(99, 2, b"filebytes");
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
        use crate::ipc::stream::StreamControl;
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
        use crate::ipc::stream::{StreamControl, encode_bulk_chunk};
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

    #[test]
    fn ordered_batch_routes_to_intact_bytes() {
        // 회귀(Gate4) end-to-end: begin+chunk0+chunk1+commit 이 **한 pump 배치**에
        // 함께 도착 → 라우팅이 bulk_events 를 순서대로 레지스트리에 반영하면 최종
        // take 된 bytes 가 전량 온전해야 한다. (분리 벡터 시절엔 chunk pass 가 begin
        // pass 보다 먼저 돌아 청크가 미등록 transfer 로 폐기 → 빈 파일이 저장됐다.)
        use crate::core::bulk_transfer::BulkTransferRegistry;
        use crate::ipc::stream::{StreamControl, encode_bulk_chunk};

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

        // 라우팅(boot.rs/event_handler.rs)이 하는 것과 동형: 단일 벡터를 순서대로 처리.
        let mut reg = BulkTransferRegistry::new();
        let mut committed: Option<(String, Vec<u8>)> = None;
        for (client_id, event) in out.bulk_events {
            match event {
                BulkEvent::Begin {
                    transfer_id,
                    filename,
                    total_size,
                } => reg.begin(client_id, transfer_id, filename, total_size),
                BulkEvent::Chunk {
                    transfer_id,
                    seq,
                    bytes,
                } => {
                    assert!(
                        reg.append(client_id, transfer_id, seq, &bytes),
                        "chunk must land on a registered transfer (begin already processed)"
                    );
                }
                BulkEvent::Commit { transfer_id } => {
                    committed = reg.take(client_id, transfer_id);
                }
            }
        }
        assert_eq!(
            committed,
            Some(("f.bin".to_string(), b"abcdef".to_vec())),
            "commit 시 누적 bytes 가 전량 온전해야 한다"
        );
    }

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
