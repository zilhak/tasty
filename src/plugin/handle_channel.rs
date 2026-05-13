//! 호스트 측 보조 핸들 채널.
//!
//! 메인 TCP 채널([`crate::plugin::listener::HostListener`])은 fd/HANDLE을 운반할 수
//! 없으므로, 보조 채널을 별도로 둔다. Unix는 `AF_UNIX` socket, Windows는 Named Pipe.
//!
//! 02b 단계에서는 두 가지 동작만 검증한다:
//! 1. plugin이 endpoint로 connect → [`crate::plugin::protocol::AuthMessage`] 한 줄 송신.
//! 2. host가 토큰 검증 후 [`tasty_plugin_protocol::AuthAckEnvelope`] 응답, 보조 채널
//!    소켓을 `expect_connection`을 호출한 caller에게 분배.
//!
//! 02c에서 이 [`HandleStream`] 위로 SCM_RIGHTS/DuplicateHandle 핸들 전송이 추가될
//! 예정.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use tasty_plugin_protocol::{AuthAck, AuthAckEnvelope};

use crate::plugin::protocol::AuthMessage;

const AUTH_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// 호스트 ↔ plugin 보조 채널의 OS-네이티브 stream 추상.
///
/// Unix는 [`std::os::unix::net::UnixStream`], Windows는 Named Pipe handle을 감싼다.
/// 02b에서는 단순히 NDJSON 라인 송수신만 노출되고, 02c에서 ancillary data 송수신
/// API가 추가된다.
pub struct HandleStream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: platform::PipeServerStream,
}

impl HandleStream {
    /// 한 줄을 NDJSON으로 송신.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut buf = line.as_bytes().to_vec();
        buf.push(b'\n');
        self.write_all(&buf)
    }

    /// 임의 바이트 송신. 02c에서 핸들 직후 ancillary data와 함께 호출됨.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.write_all(bytes)?;
            self.inner.flush()
        }
        #[cfg(windows)]
        {
            self.inner.write_all(bytes)?;
            self.inner.flush()
        }
    }
}

#[cfg(unix)]
impl HandleStream {
    fn from_unix(stream: std::os::unix::net::UnixStream) -> Self {
        Self { inner: stream }
    }
}

#[cfg(windows)]
impl HandleStream {
    #[allow(dead_code)]
    fn from_pipe(stream: platform::PipeServerStream) -> Self {
        Self { inner: stream }
    }
}

/// 보조 채널 listener. 호스트 부팅 시 한 번만 bind한다.
///
/// accept 스레드 하나가 모든 incoming connection을 받고, plugin이 보낸 첫 줄의
/// `AuthMessage`로 토큰을 매칭한 뒤 [`HandleListener::expect_connection`]을 호출한
/// caller에게 stream을 분배한다.
///
/// Windows에서는 02c까지 stub이다 — [`HandleListener::bind`]가 `Unsupported`를 반환.
pub struct HandleListener {
    endpoint: String,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>>,
    /// Drop에서 socket file 정리하기 위한 path 보관. Windows에서는 사용하지 않는다.
    #[cfg(unix)]
    _socket_path: std::path::PathBuf,
    _accept_thread: std::thread::JoinHandle<()>,
}

impl HandleListener {
    /// 보조 채널을 bind. Unix는 임시 socket 파일을 만들고, Windows는 Named Pipe를 연다.
    #[cfg(unix)]
    pub fn bind() -> io::Result<Self> {
        use std::os::unix::net::UnixListener;
        use std::time::SystemTime;

        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let socket_path = std::env::temp_dir()
            .join(format!("tasty-handle-{pid}-{:x}.sock", nanos as u64));

        // stale 파일이 남아 있으면 unlink. 다음 bind를 위한 idempotent 정리.
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path)?;
        let endpoint = socket_path.to_string_lossy().into_owned();

        let pending: Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>> = Arc::default();
        let pending_clone = pending.clone();
        let accept_thread = std::thread::Builder::new()
            .name("plugin-handle-listener".to_string())
            .spawn(move || {
                for incoming in listener.incoming() {
                    match incoming {
                        Ok(stream) => handle_incoming_unix(stream, &pending_clone),
                        Err(e) => {
                            tracing::warn!("handle channel accept error: {e}");
                        }
                    }
                }
            })?;

        Ok(Self {
            endpoint,
            pending,
            _socket_path: socket_path,
            _accept_thread: accept_thread,
        })
    }

    /// 보조 채널을 bind. Windows 구현은 02c에서 채워진다 — 현재는 `Unsupported`.
    #[cfg(windows)]
    pub fn bind() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel on Windows is not implemented yet (Step 02c)",
        ))
    }

    /// plugin spawn에 전달할 endpoint 문자열. Unix는 socket path, Windows는 pipe 이름.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 해당 token으로 connect할 plugin의 stream을 기다린다. `timeout` 안에 안 오면 `None`.
    pub fn expect_connection(&self, token: &str, timeout: Duration) -> Option<HandleStream> {
        let rx = self.register_token(token);
        match rx.recv_timeout(timeout) {
            Ok(stream) => Some(stream),
            Err(_) => {
                self.cancel_token(token);
                None
            }
        }
    }

    /// 해당 token에 대한 mailbox를 등록하고 stream receiver를 반환한다. blocking 없이
    /// 즉시 반환하므로, plugin spawn이 N개 직렬로 일어나는 상황에서 startup 지연을
    /// 일으키지 않는다. 호출자는 [`HandleListener::cancel_token`]으로 mailbox 정리
    /// 책임을 가지거나, 자연히 Receiver가 drop될 때까지 둔다 (다음 accept 시 SendError로
    /// 자동 정리).
    pub fn register_token(&self, token: &str) -> mpsc::Receiver<HandleStream> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(token.to_string(), tx);
        }
        rx
    }

    /// 미사용 mailbox 명시적 제거. plugin 종료 시 호출 가능.
    #[allow(dead_code)]
    pub fn cancel_token(&self, token: &str) {
        if let Ok(mut p) = self.pending.lock() {
            p.remove(token);
        }
    }
}

#[cfg(unix)]
impl Drop for HandleListener {
    fn drop(&mut self) {
        // 임시 socket 파일 정리. listener thread는 process exit과 함께 사라진다.
        let _ = std::fs::remove_file(&self._socket_path);
    }
}

#[cfg(unix)]
fn handle_incoming_unix(
    stream: std::os::unix::net::UnixStream,
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>>,
) {
    let _ = stream.set_read_timeout(Some(AUTH_READ_TIMEOUT));
    let cloned = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("handle channel: stream clone failed: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line) {
        tracing::warn!("handle channel: auth read failed: {e}");
        return;
    }
    let auth: AuthMessage = match serde_json::from_str(line.trim()) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("handle channel: invalid auth message: {e}");
            return;
        }
    };
    let _ = stream.set_read_timeout(None);

    let tx_opt = pending
        .lock()
        .ok()
        .and_then(|mut p| p.remove(&auth.token));

    match tx_opt {
        Some(tx) => {
            if let Err(e) = send_auth_ack_unix(&stream, true, None) {
                tracing::warn!(
                    "handle channel: plugin '{}' auth_ack send failed: {e} — dropping",
                    auth.plugin_id
                );
                return;
            }
            tracing::debug!(
                "handle channel: plugin '{}' authenticated on aux channel",
                auth.plugin_id
            );
            let _ = tx.send(HandleStream::from_unix(stream));
        }
        None => {
            tracing::warn!(
                "handle channel: auth with unknown/expired token (plugin_id={})",
                auth.plugin_id
            );
            if let Err(e) = send_auth_ack_unix(&stream, false, Some("token mismatch")) {
                tracing::debug!("handle channel: auth_ack(false) send failed: {e}");
            }
        }
    }
}

#[cfg(unix)]
fn send_auth_ack_unix(
    stream: &std::os::unix::net::UnixStream,
    ok: bool,
    reason: Option<&str>,
) -> io::Result<()> {
    let env = AuthAckEnvelope {
        auth_ack: AuthAck {
            ok,
            reason: reason.map(|s| s.to_string()),
        },
    };
    let line = serde_json::to_string(&env)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut w = stream;
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

// Windows Named Pipe 구현은 02c에서 채워진다. 02b에서는 module이 빈 placeholder를
// 가지지만, type 참조가 컴파일되도록 stub 타입만 둔다.
#[cfg(windows)]
mod platform {
    use std::io::{self, Read, Write};

    /// Named Pipe server-side stream의 placeholder. 02c에서 실제 HANDLE 래퍼로 교체.
    pub(super) struct PipeServerStream;

    impl Write for PipeServerStream {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle channel write not implemented on Windows yet",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for PipeServerStream {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle channel read not implemented on Windows yet",
            ))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    #[test]
    fn handle_listener_bind_produces_endpoint() {
        let l = HandleListener::bind().expect("bind");
        assert!(!l.endpoint().is_empty());
        assert!(std::path::Path::new(l.endpoint()).exists());
    }

    #[test]
    fn handle_listener_drop_removes_socket_file() {
        let path: std::path::PathBuf;
        {
            let l = HandleListener::bind().expect("bind");
            path = std::path::PathBuf::from(l.endpoint());
            assert!(path.exists());
        }
        assert!(!path.exists(), "socket file should be removed on Drop");
    }

    #[test]
    fn auth_flow_matches_token() {
        let listener = HandleListener::bind().expect("bind");
        let endpoint = listener.endpoint().to_string();
        let token = "test-handle-token".to_string();

        std::thread::scope(|s| {
            let token_clone = token.clone();
            let endpoint_clone = endpoint.clone();
            s.spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let mut stream = UnixStream::connect(&endpoint_clone).unwrap();
                let auth = AuthMessage {
                    plugin_id: "com.test.plugin".into(),
                    token: token_clone,
                };
                let line = serde_json::to_string(&auth).unwrap() + "\n";
                stream.write_all(line.as_bytes()).unwrap();
                stream.flush().unwrap();
                // ack 한 줄 read해서 채널이 살아 있음을 확인.
                let cloned = stream.try_clone().unwrap();
                let mut reader = BufReader::new(cloned);
                let mut ack = String::new();
                reader.read_line(&mut ack).unwrap();
                assert!(ack.contains("\"ok\":true"));
                std::thread::sleep(Duration::from_millis(50));
            });

            let stream = listener.expect_connection(&token, Duration::from_secs(2));
            assert!(stream.is_some(), "expected handle stream to be received");
        });
    }

    #[test]
    fn auth_flow_rejects_unknown_token() {
        let listener = HandleListener::bind().expect("bind");
        let endpoint = listener.endpoint().to_string();

        std::thread::scope(|s| {
            let endpoint_clone = endpoint.clone();
            s.spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let mut stream = UnixStream::connect(&endpoint_clone).unwrap();
                let auth = AuthMessage {
                    plugin_id: "com.test.plugin".into(),
                    token: "unknown-token".into(),
                };
                let line = serde_json::to_string(&auth).unwrap() + "\n";
                let _ = stream.write_all(line.as_bytes());
                let _ = stream.flush();
                let cloned = stream.try_clone().unwrap();
                let mut reader = BufReader::new(cloned);
                let mut ack = String::new();
                let _ = reader.read_line(&mut ack);
                assert!(ack.contains("\"ok\":false"));
            });

            let stream =
                listener.expect_connection("expected-token", Duration::from_millis(800));
            assert!(stream.is_none(), "expected no stream (token mismatch)");
        });
    }
}
