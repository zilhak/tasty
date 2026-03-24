use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crate::settings::tasty_home;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
}

/// Callback to wake the main event loop when an IPC command arrives.
pub type IpcWaker = Arc<dyn Fn() + Send + Sync>;

/// IPC server that listens for JSON-RPC requests over TCP.
pub struct IpcServer {
    command_rx: mpsc::Receiver<IpcCommand>,
    port: u16,
    /// Shutdown flag to signal the accept thread to stop.
    shutdown: Arc<AtomicBool>,
    /// Custom port file path (overrides default if set).
    custom_port_file: Option<std::path::PathBuf>,
}

impl IpcServer {
    /// Start the IPC server with an optional custom port file path and waker.
    /// The waker is called whenever an IPC command is enqueued, so the event
    /// loop can wake up and process it immediately.
    pub fn start_with_port_file(port_file: Option<String>, waker: Option<IpcWaker>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        let custom_port_file = port_file.map(std::path::PathBuf::from);

        // Write port file so CLI clients can find us
        Self::write_port_file_to(port, custom_port_file.as_deref())?;

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Accept connections in a background thread with non-blocking + shutdown check
        let shutdown_clone = shutdown.clone();
        listener.set_nonblocking(true)?;
        thread::spawn(move || {
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let cmd_tx = cmd_tx.clone();
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
            port,
            shutdown,
            custom_port_file,
        })
    }

    /// Try to receive a pending IPC command (non-blocking).
    pub fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        self.command_rx.try_recv()
    }

    /// Get the port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    fn handle_connection(
        stream: std::net::TcpStream,
        cmd_tx: mpsc::Sender<IpcCommand>,
        waker: Option<IpcWaker>,
    ) {
        let peer = stream.peer_addr().ok();
        tracing::debug!("IPC client connected from {:?}", peer);

        let reader = BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        });
        let mut writer = stream;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
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
                    let _ = writeln!(writer, "{}", serde_json::to_string(&err_resp).unwrap());
                    let _ = writer.flush();
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
                break; // Main thread shut down
            }

            // Wake the event loop so it processes the command immediately
            if let Some(ref waker) = waker {
                waker();
            }

            // Wait for response from main thread
            match resp_rx.recv() {
                Ok(response) => {
                    let json = serde_json::to_string(&response).unwrap();
                    if writeln!(writer, "{}", json).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        tracing::debug!("IPC client disconnected from {:?}", peer);
    }

    /// Write the port number to a file. Uses custom path if provided, otherwise default.
    fn write_port_file_to(port: u16, custom_path: Option<&std::path::Path>) -> Result<()> {
        let path = match custom_path {
            Some(p) => p.to_path_buf(),
            None => match Self::port_file_path() {
                Some(p) => p,
                None => return Ok(()),
            },
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, port.to_string())?;
        tracing::info!("Wrote port file: {}", path.display());
        Ok(())
    }

    /// Get the path to the port file: ~/.tasty/tasty.port
    pub fn port_file_path() -> Option<std::path::PathBuf> {
        tasty_home().map(|dir| dir.join("tasty.port"))
    }

    /// Read the port from the port file (used by CLI client).
    pub fn read_port_file() -> Result<u16> {
        Self::read_port_file_from(None)
    }

    /// Read the port from a specific port file path, or the default if None.
    pub fn read_port_file_from(port_file: Option<&str>) -> Result<u16> {
        let path = match port_file {
            Some(p) => std::path::PathBuf::from(p),
            None => Self::port_file_path()
                .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?,
        };
        let contents = std::fs::read_to_string(&path)
            .map_err(|_| anyhow::anyhow!(
                "No running tasty instance found (port file not found at {})",
                path.display()
            ))?;
        contents
            .trim()
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("Invalid port file contents"))
    }

    /// Get the effective port file path for this instance.
    fn effective_port_file_path(&self) -> Option<std::path::PathBuf> {
        self.custom_port_file.clone().or_else(Self::port_file_path)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Signal the accept thread to stop
        self.shutdown.store(true, Ordering::Relaxed);
        // Clean up port file
        if let Some(path) = self.effective_port_file_path() {
            let _ = std::fs::remove_file(path);
        }
    }
}
