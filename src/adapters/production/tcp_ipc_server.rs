//! Production adapter — `TcpIpcServer`. TCP listener + mpsc channel + accept
//! thread 로 JSON-RPC 요청을 받는다. Hub 가 `Box<dyn IpcServerPort>` 로 보유.
//!
//! 옛 `src/adapters/ipc/server.rs::IpcServer` 의 본문이 D.3.D.2.b 에서 이곳으로
//! 정식 이전. wire 타입 (`IpcCommand` / `IpcWaker` / `send_response`) 은 옛
//! 위치 (`crate::ipc::server`) 에 잔존 — wire 형식과 강결합이라 trait 옆이 아니라
//! wire 모듈에 두는 게 자연스럽다 (verify 자율 결정).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::adapters::production::stream_hub::{StreamContext, StreamInbound};
use crate::ipc::port_file;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::{IpcCommand, IpcWaker};
use crate::ipc::stream::{self, StreamAck, StreamFrame, StreamTag};
use crate::ports::ipc_server::IpcServerPort;

/// TCP-backed IPC server. listening on 127.0.0.1:{dynamic} + writing port to
/// `~/.tasty/tasty.port` 등 외부 통신 표면.
pub struct TcpIpcServer {
    command_rx: mpsc::Receiver<IpcCommand>,
    /// Sender 사본 — host→plugin sync dispatch 시 외부 thread 가 직접 push.
    command_tx: mpsc::Sender<IpcCommand>,
    port: u16,
    /// Shutdown flag to signal the accept thread to stop.
    shutdown: Arc<AtomicBool>,
    /// Custom port file path (overrides default if set).
    custom_port_file: Option<std::path::PathBuf>,
}

impl TcpIpcServer {
    /// Start the IPC server with an optional custom port file path and waker.
    /// The waker is called whenever an IPC command is enqueued, so the event
    /// loop can wake up and process it immediately.
    pub fn start_with_port_file(
        port_file_override: Option<String>,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        let custom_port_file = port_file_override.map(std::path::PathBuf::from);

        // Write port file so CLI clients can find us
        port_file::write_port_file_to(port, custom_port_file.as_deref())?;

        // 이 인스턴스가 자기 데이터 루트의 주인이 된 순간, 과거 완료 알림 로그
        // (`notify/`)를 통째로 청소한다. surface_id 는 재시작마다 새로 발급되므로
        // 이전 프로세스가 남긴 로그 파일들은 이 인스턴스에선 모두 죽은 surface 의
        // 것 — 읽을 reader 가 없다. 부팅 latency 에 영향을 주지 않도록 결과를
        // 기다리지 않는 fire-and-forget 으로 던진다(다음 append 시 create_dir_all
        // 이 알아서 재생성한다).
        Self::spawn_notify_dir_cleanup();

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Accept connections in a background thread with non-blocking + shutdown check
        let shutdown_clone = shutdown.clone();
        let accept_tx = cmd_tx.clone();
        listener.set_nonblocking(true)?;
        thread::spawn(move || {
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let cmd_tx = accept_tx.clone();
                        let waker = waker.clone();
                        let stream_ctx = stream_ctx.clone();
                        thread::spawn(move || {
                            Self::handle_connection(stream, cmd_tx, waker, stream_ctx);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("IPC accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            command_rx: cmd_rx,
            command_tx: cmd_tx,
            port,
            shutdown,
            custom_port_file,
        })
    }

    /// 자기 데이터 루트 밑 `notify/` 디렉토리를 별도 스레드에서 통째로 삭제한다
    /// (fire-and-forget — join 하지 않아 부팅 흐름을 막지 않는다). 홈 미확인 시
    /// 아무것도 하지 않는다.
    fn spawn_notify_dir_cleanup() {
        let Some(dir) = tasty_utils::path::tasty_home().map(|home| home.join("notify")) else {
            return;
        };
        thread::spawn(move || Self::clear_notify_dir(&dir));
    }

    /// `notify/` 디렉토리를 통째로 삭제한다. 디렉토리가 애초에 없으면(NotFound)
    /// 정상 상황이라 무시하고, 그 외 에러만 로그한다. 재생성은 하지 않는다 —
    /// 다음 `append_notify_line` 이 `create_dir_all` 로 알아서 만든다.
    fn clear_notify_dir(dir: &std::path::Path) {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!("failed to clear notify dir {}: {}", dir.display(), e);
            }
        }
    }

    fn handle_connection(
        stream: std::net::TcpStream,
        cmd_tx: mpsc::Sender<IpcCommand>,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
    ) {
        let peer = stream.peer_addr().ok();
        tracing::debug!("IPC client connected from {:?}", peer);

        // Listener is non-blocking for polling accept(), but each connection
        // needs blocking I/O for the request-response loop.
        if let Err(e) = stream.set_nonblocking(false) {
            tracing::warn!("Failed to set stream to blocking mode: {}", e);
            return;
        }

        let mut reader = BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        });
        let mut writer = stream;

        // Read the first line manually so the BufReader retains any bytes
        // buffered after it. On a streaming-channel upgrade those buffered bytes
        // are the start of the binary frames following the handshake line.
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                tracing::debug!("IPC client disconnected (eof) {:?}", peer);
                return;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("IPC read error from {:?}: {}", peer, e);
                return;
            }
        }

        // Streaming upgrade: first line is `{"method":"stream.open",...}`. The
        // connection leaves the request-response model and becomes a framed
        // bidirectional pipe.
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(line.trim())
            && req.method == stream::STREAM_OPEN_METHOD
        {
            Self::handle_stream_connection(reader, writer, req, stream_ctx, peer);
            return;
        }

        // Normal request-response: handle the first line, then keep reading.
        if Self::process_request_line(&line, &cmd_tx, &waker, &mut writer, peer) {
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("IPC read error from {:?}: {}", peer, e);
                        break;
                    }
                }
                if !Self::process_request_line(&line, &cmd_tx, &waker, &mut writer, peer) {
                    break;
                }
            }
        }

        tracing::debug!("IPC client disconnected from {:?}", peer);
    }

    /// Drive an upgraded streaming connection: a write thread drains the client's
    /// push sink to the socket, while this thread reads inbound frames and
    /// forwards them to the main loop. Returns when the client detaches or the
    /// socket closes.
    ///
    /// No authentication: the streaming channel trusts SSH + 127.0.0.1 loopback
    /// (decisions.md #5). `session_token` in the handshake is ignored.
    fn handle_stream_connection(
        mut reader: BufReader<std::net::TcpStream>,
        writer: std::net::TcpStream,
        req: JsonRpcRequest,
        ctx: StreamContext,
        peer: Option<std::net::SocketAddr>,
    ) {
        // 조용한(FIN/RST 없는) 네트워크 단절 감지용 read timeout. reader/writer 는 같은
        // 소켓의 clone(둘 다 원래 `stream`에서 파생)이라 옵션이 공유돼 write 쪽에는 영향
        // 없다 — read 만 타임아웃 대상. write thread(아래)가 이 주기 이내에 Ping 을 흘려
        // idle 세션에서도 상대측 read timeout 이 갱신되게 한다.
        if let Err(e) = reader
            .get_ref()
            .set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT))
        {
            tracing::warn!("stream client: failed to set read timeout: {e}");
        }

        let client_id = ctx.hub.alloc_id();
        let sink_rx = ctx.hub.register(client_id);
        tracing::debug!("stream client {} upgraded from {:?}", client_id, peer);

        // attach 대상을 핸드셰이크 params 에서 추출. surface(단계 4) 또는 workspace
        // (단계 6) 둘 중 하나. workspace 우선(둘 다 지정은 비정상이지만 안전 분기).
        let open_params = serde_json::from_value::<stream::StreamOpenParams>(req.params).ok();
        let attach_target = open_params.as_ref().and_then(|p| p.target);
        let attach_workspace = open_params.as_ref().and_then(|p| p.target_workspace);
        // bulk 전용 연결(ADR-0053): 이 연결은 mirror/attach 를 하지 않고(= holder 가
        // 되지 않고) 파일 청크만 나른다. 여기서 hub 에 bulk 로 태깅하면 read 루프가
        // 프레임을 보내기 전에 결속이 서므로, 이후 pump_inbound 가 이 연결의 Data 를
        // 파일 청크로 분류한다(연결-단위 태깅). attach 분기와 상호배타.
        let bulk_workspace = open_params.as_ref().and_then(|p| p.bulk_workspace);
        if let Some(ws) = bulk_workspace {
            ctx.hub.register_bulk(client_id, ws);
        }

        // Write thread: drain the push sink (fed by the main loop) to the socket.
        // `recv_timeout` 대신 blocking iterator 를 쓰던 옛 구현은 sink 가 idle 이면
        // 소켓에 아무것도 안 나가 client 쪽 read timeout 이 결국 만료된다 — sink 가
        // HEARTBEAT_INTERVAL 동안 조용하면 빈 Ping 프레임을 대신 흘려보낸다. 실제
        // Data/Control 트래픽이 있으면 그 자체가 liveness 라 Ping 은 나가지 않는다.
        let mut w = writer;
        let write_handle = thread::spawn(move || {
            loop {
                match sink_rx.recv_timeout(stream::HEARTBEAT_INTERVAL) {
                    Ok(frame) => {
                        if stream::write_frame(&mut w, frame.tag, &frame.payload).is_err() {
                            break;
                        }
                        if frame.tag == StreamTag::Detach {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if stream::write_frame(&mut w, StreamTag::Ping, &[]).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break, // unregister 됨
                }
            }
        });

        // Handshake ack — pushed through the sink so the single write thread owns
        // all socket writes.
        let ack = StreamAck {
            ok: true,
            client_id: Some(client_id),
            proto: stream::STREAM_PROTO,
            error: None,
        };
        let ack_bytes = serde_json::to_vec(&ack).unwrap_or_default();
        let _ = ctx // best-effort ack push — PushResult(Result 아님) 무시: client 끊겼으면 무해.
            .hub
            .push(client_id, StreamFrame::new(StreamTag::Control, ack_bytes));

        // attach 요청이면 메인루프로 위임(엔진은 메인루프 단일소유 → accept thread 가
        // 직접 acquire 불가). 메인루프가 lock 획득 + 스냅샷 push + 출력 tap 결선한다.
        // attach 결과(성공/거부)는 별도 Control 프레임으로 client 에 통지된다.
        if let Some(target_workspace_id) = attach_workspace {
            if ctx
                .inbound_tx
                .send(StreamInbound::AttachWorkspaceRequest {
                    client_id,
                    target_workspace_id,
                })
                .is_ok()
            {
                (ctx.waker)();
            }
        } else if let Some(target_surface_id) = attach_target
            && ctx
                .inbound_tx
                .send(StreamInbound::AttachRequest {
                    client_id,
                    target_surface_id,
                })
                .is_ok()
        {
            (ctx.waker)();
        }

        // Read loop: forward inbound frames to the main loop (which echoes them
        // back in debug builds; later steps interpret them as input/resize).
        loop {
            match stream::read_frame(&mut reader) {
                Ok(frame) if frame.tag == StreamTag::Detach => break,
                Ok(frame) => {
                    if ctx
                        .inbound_tx
                        .send(StreamInbound::Frame { client_id, frame })
                        .is_err()
                    {
                        break; // main loop gone
                    }
                    (ctx.waker)();
                }
                Err(_) => break, // EOF / oversize / unknown tag
            }
        }

        ctx.hub.unregister(client_id); // drops the sink sender → write thread exits
        let _ = write_handle.join(); // writer 스레드 join 실패(패닉) 무시 — 종료 경로
        // Notify the main loop so it releases any attach locks this client held
        // (attach/detach step 3). Best-effort: if the main loop is gone, nothing
        // to release anyway.
        if ctx
            .inbound_tx
            .send(StreamInbound::Disconnected { client_id })
            .is_ok()
        {
            (ctx.waker)();
        }
        tracing::debug!("stream client {} disconnected from {:?}", client_id, peer);
    }

    /// Handle one request line of a request-response connection. Returns `false`
    /// when the connection should be torn down (send/recv/write failure), `true`
    /// to keep reading (including for empty or unparseable lines).
    fn process_request_line(
        line: &str,
        cmd_tx: &mpsc::Sender<IpcCommand>,
        waker: &Option<IpcWaker>,
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return true;
        }

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                if let Err(e) = writeln!(writer, "{}", serde_json::to_string(&err_resp).unwrap()) {
                    tracing::trace!("IPC parse-error response write failed: {e}");
                }
                if let Err(e) = writer.flush() {
                    tracing::trace!("IPC parse-error response flush failed: {e}");
                }
                return true;
            }
        };

        // Create a response channel for this request
        let (resp_tx, resp_rx) = mpsc::sync_channel(1);

        let cmd = IpcCommand {
            request,
            response_tx: resp_tx,
        };

        // Send command to main thread
        if cmd_tx.send(cmd).is_err() {
            tracing::warn!("IPC cmd_tx.send failed (main thread shut down?)");
            return false;
        }

        // Wake the event loop so it processes the command immediately
        if let Some(waker) = waker {
            waker();
        }

        // Wait for response from main thread
        match resp_rx.recv() {
            Ok(response) => {
                let json = serde_json::to_string(&response).unwrap();
                if let Err(e) = writeln!(writer, "{}", json) {
                    tracing::warn!("IPC write error: {}", e);
                    return false;
                }
                if let Err(e) = writer.flush() {
                    tracing::warn!("IPC flush error: {}", e);
                    return false;
                }
                tracing::debug!("IPC response sent for {:?}", peer);
                true
            }
            Err(e) => {
                tracing::warn!(
                    "IPC resp_rx.recv failed: {} (response_tx dropped without sending)",
                    e
                );
                false
            }
        }
    }

    /// Get the effective port file path for this instance.
    fn effective_port_file_path(&self) -> Option<std::path::PathBuf> {
        self.custom_port_file
            .clone()
            .or_else(port_file::port_file_path)
    }
}

impl IpcServerPort for TcpIpcServer {
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        self.command_rx.try_recv()
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn command_sender(&self) -> mpsc::Sender<IpcCommand> {
        self.command_tx.clone()
    }
}

#[cfg(test)]
mod notify_cleanup_tests {
    use super::*;

    // 더미 파일이 든 notify/ 를 clear 하면 디렉토리가 통째로 사라진다.
    #[test]
    fn clear_removes_populated_notify_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        for id in ["11.log", "42.log", "1337.log"] {
            std::fs::write(notify.join(id), b"spawn done\n").unwrap();
        }
        assert!(notify.exists());

        TcpIpcServer::clear_notify_dir(&notify);

        assert!(!notify.exists(), "notify dir should be removed");
    }

    // 애초에 없는 디렉토리를 clear 해도 에러/패닉 없이 no-op.
    #[test]
    fn clear_is_noop_when_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        assert!(!notify.exists());

        // NotFound 를 삼키므로 패닉 없이 반환해야 한다.
        TcpIpcServer::clear_notify_dir(&notify);

        assert!(!notify.exists());
    }

    // fire-and-forget spawn 경로도 스레드를 join 해 실제로 비워지는지 확인.
    #[test]
    fn spawned_thread_clears_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        std::fs::write(notify.join("7.log"), b"x\n").unwrap();

        // spawn_notify_dir_cleanup 은 tasty_home() 에 의존하므로 여기선 직접
        // 스레드를 띄워 프로덕션과 동일한 fire-and-forget 경로(spawn→clear)를
        // 재현하고, 테스트에서만 join 으로 완료를 기다린다.
        let dir = notify.clone();
        let handle = thread::spawn(move || TcpIpcServer::clear_notify_dir(&dir));
        handle.join().unwrap();

        assert!(!notify.exists());
    }
}

impl Drop for TcpIpcServer {
    fn drop(&mut self) {
        // Signal the accept thread to stop
        self.shutdown.store(true, Ordering::Relaxed);
        // Clean up port file. 파일이 이미 사라졌거나 권한이 없는 케이스도 정상 종료
        // 흐름에서 발생 가능 — trace 레벨로만 기록한다.
        if let Some(path) = self.effective_port_file_path()
            && let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!("port file {} remove failed: {e}", path.display());
        }
    }
}
