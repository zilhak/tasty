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

use crate::ipc::port_file;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::{IpcCommand, IpcWaker};
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
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        let custom_port_file = port_file_override.map(std::path::PathBuf::from);

        // Write port file so CLI clients can find us
        port_file::write_port_file_to(port, custom_port_file.as_deref())?;

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
                        thread::spawn(move || {
                            Self::handle_connection(stream, cmd_tx, waker);
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

    fn handle_connection(
        stream: std::net::TcpStream,
        cmd_tx: mpsc::Sender<IpcCommand>,
        waker: Option<IpcWaker>,
    ) {
        let peer = stream.peer_addr().ok();
        tracing::debug!("IPC client connected from {:?}", peer);

        // Listener is non-blocking for polling accept(), but each connection
        // needs blocking I/O for the request-response loop.
        if let Err(e) = stream.set_nonblocking(false) {
            tracing::warn!("Failed to set stream to blocking mode: {}", e);
            return;
        }

        let reader = BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        });
        let mut writer = stream;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("IPC read error from {:?}: {}", peer, e);
                    break;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
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
                    if let Err(e) =
                        writeln!(writer, "{}", serde_json::to_string(&err_resp).unwrap())
                    {
                        tracing::trace!("IPC parse-error response write failed: {e}");
                    }
                    if let Err(e) = writer.flush() {
                        tracing::trace!("IPC parse-error response flush failed: {e}");
                    }
                    continue;
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
                break;
            }

            // Wake the event loop so it processes the command immediately
            if let Some(ref waker) = waker {
                waker();
            }

            // Wait for response from main thread
            match resp_rx.recv() {
                Ok(response) => {
                    let json = serde_json::to_string(&response).unwrap();
                    if let Err(e) = writeln!(writer, "{}", json) {
                        tracing::warn!("IPC write error: {}", e);
                        break;
                    }
                    if let Err(e) = writer.flush() {
                        tracing::warn!("IPC flush error: {}", e);
                        break;
                    }
                    tracing::debug!("IPC response sent for {:?}", peer);
                }
                Err(e) => {
                    tracing::warn!(
                        "IPC resp_rx.recv failed: {} (response_tx dropped without sending)",
                        e
                    );
                    break;
                }
            }
        }

        tracing::debug!("IPC client disconnected from {:?}", peer);
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

impl Drop for TcpIpcServer {
    fn drop(&mut self) {
        // Signal the accept thread to stop
        self.shutdown.store(true, Ordering::Relaxed);
        // Clean up port file. 파일이 이미 사라졌거나 권한이 없는 케이스도 정상 종료
        // 흐름에서 발생 가능 — trace 레벨로만 기록한다.
        if let Some(path) = self.effective_port_file_path() {
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::trace!("port file {} remove failed: {e}", path.display());
                }
            }
        }
    }
}
