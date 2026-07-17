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
    /// Idempotent.
    pub fn unregister(&self, id: StreamClientId) {
        if let Ok(mut sinks) = self.sinks.lock() {
            sinks.remove(&id);
        }
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
                    match frame.tag {
                        crate::ipc::stream::StreamTag::Data => {
                            out.input_frames.push((client_id, frame.payload));
                        }
                        crate::ipc::stream::StreamTag::Control => {
                            // Client→server Control messages: `StructuralOp`
                            // (split/new-tab/close/move forward) and `ClientResize`
                            // (client-driven mirror geometry). Any other
                            // `StreamControl` variant (server→client only) is
                            // ignored; a payload that isn't a `StreamControl` at
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
                                Ok(_) => {}
                                Err(_) => {
                                    if let Ok(msg) =
                                        serde_json::from_slice::<CaptureUploadMsg>(&frame.payload)
                                    {
                                        out.capture_uploads.push((client_id, msg));
                                    }
                                }
                            }
                        }
                        // Ping/Detach from clients carry no step-4 payload.
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
