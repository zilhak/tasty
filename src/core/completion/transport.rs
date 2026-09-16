//! Bounded existing-endpoint transport. Never starts a Codex daemon.
use super::deadline::{BudgetStream, Deadline};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_FRAME: u64 = 8 * 1024 * 1024;
pub struct Transport {
    socket: Socket,
    deadline: Deadline,
}
enum Socket {
    #[cfg(unix)]
    Unix(Box<WebSocket<BudgetStream<std::os::unix::net::UnixStream>>>),
    Web(Box<WebSocket<MaybeTlsStream<BudgetStream<TcpStream>>>>),
}
pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    if let Some(path) = endpoint.strip_prefix("unix://") {
        if !cfg!(unix) {
            bail!("unix_transport_unsupported_on_this_platform");
        }
        if !std::path::Path::new(path).is_absolute() {
            bail!("explicit_absolute_socket_required");
        }
        return Ok(());
    }
    if endpoint.starts_with("ws://127.0.0.1:")
        || endpoint.starts_with("ws://localhost:")
        || endpoint.starts_with("wss://")
    {
        if endpoint.contains('@') || endpoint.contains('?') || endpoint.contains('#') {
            bail!("endpoint_must_not_contain_credentials_or_query");
        }
        #[cfg(windows)]
        if endpoint.chars().any(|c| {
            matches!(
                c,
                '"' | '\'' | '$' | '`' | '&' | '|' | '<' | '>' | '^' | '%' | '!' | '\r' | '\n'
            )
        }) {
            bail!("endpoint_unsafe_for_windows_resume_shell");
        }
        return Ok(());
    }
    bail!("unsupported_endpoint: use an explicit unix socket, loopback ws, or wss")
}
impl Transport {
    pub fn connect(endpoint: &str, auth_env: Option<&str>) -> Result<Self> {
        validate_endpoint(endpoint)?;
        let budget = Deadline::new(Instant::now() + TIMEOUT);
        #[cfg(unix)]
        if let Some(path) = endpoint.strip_prefix("unix://") {
            let socket = std::os::unix::net::UnixStream::connect(path)?;
            let socket = BudgetStream {
                socket,
                deadline: budget.clone(),
            };
            let (mut socket, _) = tungstenite::client("ws://localhost/", socket)?;
            limit_frames(&mut socket);
            return Ok(Self {
                socket: Socket::Unix(Box::new(socket)),
                deadline: budget,
            });
        }
        // Resolve and connect with a deadline before websocket/TLS negotiation.
        let uri: tungstenite::http::Uri = endpoint.parse()?;
        let host = uri.host().context("endpoint host missing")?;
        let port = uri.port_u16().unwrap_or(if endpoint.starts_with("wss:") {
            443
        } else {
            80
        });
        use std::net::ToSocketAddrs;
        let deadline = std::time::Instant::now() + TIMEOUT;
        let mut connected = None;
        let mut last_error = None;
        for addr in (host, port).to_socket_addrs()? {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match TcpStream::connect_timeout(&addr, remaining) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        let stream = connected.ok_or_else(|| {
            last_error.map(anyhow::Error::from).unwrap_or_else(|| {
                anyhow::anyhow!("endpoint did not resolve or connect within deadline")
            })
        })?;
        budget.reset(Instant::now() + TIMEOUT)?;
        let stream = BudgetStream {
            socket: stream,
            deadline: budget.clone(),
        };
        use tungstenite::client::IntoClientRequest;
        let mut request = endpoint.into_client_request()?;
        if let Some(name) = auth_env {
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                bail!("invalid_auth_env_reference");
            }
            let token = std::env::var(name).map_err(|_| anyhow::anyhow!("auth_env_unavailable"))?;
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid_auth_token"))?,
            );
        }
        let (mut socket, _) =
            tungstenite::client_tls(request, stream).map_err(|error| match error {
                tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                    anyhow::anyhow!(
                        "websocket_handshake_rejected_http_{}",
                        response.status().as_u16()
                    )
                }
                tungstenite::HandshakeError::Failure(tungstenite::Error::Tls(_)) => {
                    anyhow::anyhow!("tls_verification_failed")
                }
                _ => anyhow::anyhow!("websocket_handshake_failed"),
            })?;
        limit_frames(&mut socket);
        Ok(Self {
            socket: Socket::Web(Box::new(socket)),
            deadline: budget,
        })
    }
    pub fn send(&mut self, value: &Value) -> Result<()> {
        let text = serde_json::to_string(value)?;
        self.deadline.reset(Instant::now() + TIMEOUT)?;
        match &mut self.socket {
            #[cfg(unix)]
            Socket::Unix(socket) => socket.send(Message::Text(text.into()))?,
            Socket::Web(socket) => socket.send(Message::Text(text.into()))?,
        }
        Ok(())
    }
    pub fn receive_until(&mut self, response_deadline: Instant) -> Result<Value> {
        let deadline = response_deadline.min(Instant::now() + TIMEOUT);
        self.deadline.reset(deadline)?;
        match &mut self.socket {
            #[cfg(unix)]
            Socket::Unix(socket) => receive(socket, deadline),
            Socket::Web(socket) => receive(socket, deadline),
        }
    }
}
fn limit_frames<S: Read + Write>(socket: &mut WebSocket<S>) {
    socket.set_config(|config| {
        config.max_message_size = Some(MAX_FRAME as usize);
        config.max_frame_size = Some(MAX_FRAME as usize);
    });
}
fn receive<S: Read + Write>(socket: &mut WebSocket<S>, deadline: Instant) -> Result<Value> {
    loop {
        if std::time::Instant::now() >= deadline {
            bail!("app_server_frame_timeout");
        }
        match socket.read()? {
            Message::Text(text) if text.len() as u64 <= MAX_FRAME => {
                return Ok(serde_json::from_str(&text)?);
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => bail!("invalid_app_server_frame"),
        }
    }
}

/// Unix socket replacement is observable here. A TCP URL supplies no socket-file identity;
/// the protocol layer separately verifies diagnostics PID, home, and exact loaded thread.
pub fn endpoint_identity(endpoint: &str) -> Result<String> {
    #[cfg(unix)]
    if let Some(path) = endpoint.strip_prefix("unix://") {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)?;
        return Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()));
    }
    Ok("remote_socket_identity_unavailable".into())
}
