use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use directories::BaseDirs;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
}

/// IPC server that listens for JSON-RPC requests over TCP.
pub struct IpcServer {
    command_rx: mpsc::Receiver<IpcCommand>,
    port: u16,
}

impl IpcServer {
    /// Start the IPC server on a random port. Returns the server handle.
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        // Write port file so CLI clients can find us
        Self::write_port_file(port)?;

        let (cmd_tx, cmd_rx) = mpsc::channel();

        // Accept connections in a background thread
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let cmd_tx = cmd_tx.clone();
                        thread::spawn(move || {
                            Self::handle_connection(stream, cmd_tx);
                        });
                    }
                    Err(e) => {
                        tracing::warn!("IPC accept error: {}", e);
                    }
                }
            }
        });

        Ok(Self {
            command_rx: cmd_rx,
            port,
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

    /// Write the port number to a file so CLI clients can find the server.
    fn write_port_file(port: u16) -> Result<()> {
        if let Some(path) = Self::port_file_path() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, port.to_string())?;
            tracing::info!("Wrote port file: {}", path.display());
        }
        Ok(())
    }

    /// Get the path to the port file: ~/.config/tasty/tasty.port
    pub fn port_file_path() -> Option<std::path::PathBuf> {
        BaseDirs::new().map(|dirs| dirs.config_dir().join("tasty").join("tasty.port"))
    }

    /// Read the port from the port file (used by CLI client).
    pub fn read_port_file() -> Result<u16> {
        let path = Self::port_file_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
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
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Clean up port file
        if let Some(path) = Self::port_file_path() {
            let _ = std::fs::remove_file(path);
        }
    }
}
