//! 호스트 측 plugin TCP listener.
//!
//! 호스트가 부팅 시 `127.0.0.1:0` 으로 한 번만 bind, plugin이 spawn 후
//! 이 포트로 connect. 첫 줄로 `AuthMessage` (token 포함) 보내야 인증.
//!
//! plugin마다 token이 다르므로 listener는 token → 채널 맵을 들고 있다가
//! 매칭되는 spawn 측에 stream을 전달.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::plugin::protocol::{AuthAck, AuthAckEnvelope, AuthMessage};

const AUTH_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HostListener {
    addr: SocketAddr,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<TcpStream>>>>,
    _accept_thread: std::thread::JoinHandle<()>,
}

impl HostListener {
    pub fn bind() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let pending: Arc<Mutex<HashMap<String, mpsc::Sender<TcpStream>>>> = Arc::default();
        let pending_clone = pending.clone();
        let accept_thread = std::thread::Builder::new()
            .name("plugin-listener".to_string())
            .spawn(move || {
                for incoming in listener.incoming() {
                    match incoming {
                        Ok(stream) => handle_incoming(stream, &pending_clone),
                        Err(e) => {
                            tracing::warn!("plugin listener accept error: {e}");
                        }
                    }
                }
            })?;
        Ok(Self {
            addr,
            pending,
            _accept_thread: accept_thread,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// plugin spawn 직전에 호출. 해당 토큰의 connection을 받기 위한 채널을 등록.
    /// `timeout` 내 connection이 안 오면 `None`.
    pub fn expect_connection(&self, token: &str, timeout: Duration) -> Option<TcpStream> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(token.to_string(), tx);
        }
        match rx.recv_timeout(timeout) {
            Ok(stream) => Some(stream),
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(token);
                }
                None
            }
        }
    }
}

fn handle_incoming(
    stream: TcpStream,
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<TcpStream>>>>,
) {
    if let Err(e) = stream.set_read_timeout(Some(AUTH_READ_TIMEOUT)) {
        tracing::warn!("plugin listener: set_read_timeout failed: {e}");
    }
    let cloned = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("plugin listener: stream clone failed: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line) {
        tracing::warn!("plugin listener: auth read failed: {e}");
        return;
    }
    let auth: AuthMessage = match serde_json::from_str(line.trim()) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("plugin listener: invalid auth message: {e}");
            return;
        }
    };
    if let Err(e) = stream.set_read_timeout(None) {
        tracing::warn!("plugin listener: clearing read_timeout failed: {e}");
    }
    let tx_opt = pending
        .lock()
        .ok()
        .and_then(|mut p| p.remove(&auth.token));
    match tx_opt {
        Some(tx) => {
            if let Err(e) = send_auth_ack(&stream, true, None) {
                tracing::warn!(
                    "plugin '{}' auth_ack send failed: {e} — dropping",
                    auth.plugin_id
                );
                return;
            }
            tracing::info!("plugin '{}' authenticated", auth.plugin_id);
            if let Err(e) = tx.send(stream) {
                tracing::warn!("plugin '{}' stream handoff failed: {e}", auth.plugin_id);
            }
        }
        None => {
            tracing::warn!(
                "plugin auth with unknown/expired token (plugin_id={})",
                auth.plugin_id
            );
            // 명시적 거부 ack 송신 후 drop — SDK가 즉시 HandshakeRejected로 실패.
            if let Err(e) = send_auth_ack(&stream, false, Some("token mismatch")) {
                tracing::debug!("plugin auth_ack(false) send failed: {e}");
            }
        }
    }
}

fn send_auth_ack(stream: &TcpStream, ok: bool, reason: Option<&str>) -> std::io::Result<()> {
    let env = AuthAckEnvelope {
        auth_ack: AuthAck {
            ok,
            reason: reason.map(|s| s.to_string()),
        },
    };
    let line = serde_json::to_string(&env)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut w = stream;
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;

    #[test]
    fn listener_binds_on_random_port() {
        let l = HostListener::bind().unwrap();
        assert_ne!(l.port(), 0);
    }

    #[test]
    fn auth_flow_matches_token() {
        let listener = HostListener::bind().unwrap();
        let port = listener.port();
        let token = "test-token-123".to_string();

        // expect_connection in this thread (with timeout)
        std::thread::scope(|s| {
            let token_clone = token.clone();
            s.spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let auth = AuthMessage {
                    plugin_id: "com.test.plugin".into(),
                    token: token_clone,
                };
                let line = serde_json::to_string(&auth).unwrap() + "\n";
                stream.write_all(line.as_bytes()).unwrap();
                stream.flush().unwrap();
                // keep-alive to avoid premature close
                std::thread::sleep(Duration::from_millis(200));
            });

            let stream = listener.expect_connection(&token, Duration::from_secs(2));
            assert!(stream.is_some(), "expected connection to be received");
        });
    }

    #[test]
    fn auth_flow_rejects_unknown_token() {
        let listener = HostListener::bind().unwrap();
        let port = listener.port();

        std::thread::scope(|s| {
            s.spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let auth = AuthMessage {
                    plugin_id: "com.test.plugin".into(),
                    token: "unknown-token".into(),
                };
                let line = serde_json::to_string(&auth).unwrap() + "\n";
                stream.write_all(line.as_bytes()).expect("test auth write");
                stream.flush().expect("test auth flush");
            });

            let stream = listener.expect_connection("expected-token", Duration::from_millis(800));
            assert!(stream.is_none(), "expected no connection (token mismatch)");
        });
    }
}
