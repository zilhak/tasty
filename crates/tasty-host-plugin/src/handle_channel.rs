//! 호스트 측 보조 핸들 채널.
//!
//! 메인 TCP 채널([`crate::listener::HostListener`])은 fd/HANDLE을 운반할 수
//! 없으므로, 보조 채널을 별도로 둔다. Unix는 `AF_UNIX` socket, Windows는 Named Pipe.
//!
//! 02b에서 인증 핸드셰이크 + 채널 분배만 구현됐고, 02c에서 [`HandleStream::send_handle`]
//! (SCM_RIGHTS / DuplicateHandle)과 [`HandleStreamReader`](dirty 메시지 수신)가 추가됐다.

use std::collections::HashMap;
#[cfg(unix)]
use std::collections::VecDeque;
use std::io::{self, Write};
#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
#[cfg(any(unix, test))]
use std::time::Duration;

use tasty_plugin_protocol::HandleChannelMessage;
use tasty_plugin_protocol::{AuthAck, AuthAckEnvelope, AuthMessage};

#[cfg(unix)]
const AUTH_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// 호스트 ↔ plugin 보조 채널의 OS-네이티브 stream 추상.
///
/// Unix는 [`std::os::unix::net::UnixStream`], Windows는 Named Pipe handle을 감싼다.
/// 송신은 [`HandleStream::send_message`] / [`HandleStream::send_handle`]을 통해 일어나고,
/// 수신은 [`HandleStream::reader`]가 반환하는 [`HandleStreamReader`]가 담당한다.
pub struct HandleStream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: self::windows::PipeServerStream,
}

impl HandleStream {
    /// fd 없는 NDJSON 한 줄 송신 (예: ping/pong).
    pub fn send_message(&mut self, msg: &HandleChannelMessage) -> io::Result<()> {
        let line = serde_json::to_string(msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.write_line(&line)
    }

    /// 한 줄을 NDJSON으로 송신.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut buf = line.as_bytes().to_vec();
        buf.push(b'\n');
        self.write_all(&buf)
    }

    /// NDJSON 한 줄과 함께 ancillary data로 fd/HANDLE을 송신한다.
    ///
    /// `msg`는 [`HandleChannelMessage::HandleAttach`]여야 한다 (호출자가 보장).
    /// Unix는 `sendmsg(2)` + `SCM_RIGHTS`, Windows는 Named Pipe write로 직렬화된
    /// HANDLE u64를 NDJSON 라인 뒤에 이어 보낸다 (02c는 Unix만 구현).
    #[cfg(unix)]
    pub fn send_handle(
        &mut self,
        msg: &HandleChannelMessage,
        fd: std::os::fd::RawFd,
    ) -> io::Result<()> {
        debug_assert!(matches!(msg, HandleChannelMessage::HandleAttach { .. }));
        let mut line = serde_json::to_string(msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        unix_wire::send_with_fd(&self.inner, line.as_bytes(), Some(fd))
    }

    /// Windows: fd 대신 `DuplicateHandle` 결과 HANDLE u64 를 [`HandleAttach`] 의
    /// `handle` 필드에 in-band 로 실어 평범한 NDJSON 라인으로 보낸다(ancillary data 없음).
    /// `msg` 의 기존 `handle` 값은 무시하고 인자 `handle` 로 덮어써 Unix `send_handle` 과
    /// 호출 형태를 맞춘다.
    ///
    /// [`HandleAttach`]: HandleChannelMessage::HandleAttach
    #[cfg(windows)]
    pub fn send_handle(&mut self, msg: &HandleChannelMessage, handle: u64) -> io::Result<()> {
        debug_assert!(matches!(msg, HandleChannelMessage::HandleAttach { .. }));
        let msg = match msg {
            HandleChannelMessage::HandleAttach {
                request_id,
                id,
                size,
                ..
            } => HandleChannelMessage::HandleAttach {
                request_id: *request_id,
                id: *id,
                size: *size,
                handle: Some(handle),
            },
            other => other.clone(),
        };
        self.send_message(&msg)
    }

    /// 임의 바이트 송신. write_line 내부 헬퍼.
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        {
            (&self.inner).write_all(bytes)?;
            (&self.inner).flush()
        }
        #[cfg(windows)]
        {
            self.inner.write_all(bytes)?;
            self.inner.flush()
        }
    }

    /// 수신 측 reader를 분리해서 반환. write 핸들은 self에 남는다.
    /// Unix: [`std::os::unix::net::UnixStream::try_clone`]으로 fd를 dup.
    /// Windows: 미구현.
    #[cfg(unix)]
    pub fn reader(&self) -> io::Result<HandleStreamReader> {
        let cloned = self.inner.try_clone()?;
        Ok(HandleStreamReader::from_unix(cloned))
    }

    /// Windows: duplex 파이프 핸들을 복제해 reader 스레드용 stream 을 분리한다.
    #[cfg(windows)]
    pub fn reader(&self) -> io::Result<HandleStreamReader> {
        let cloned = self.inner.try_clone()?;
        Ok(HandleStreamReader::from_windows(cloned))
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
    fn from_windows(stream: self::windows::PipeServerStream) -> Self {
        Self { inner: stream }
    }
}

/// 보조 채널에서 들어오는 NDJSON 메시지를 한 줄씩 파싱해 돌려준다.
///
/// host 측에서는 [`HandleChannelMessage::Dirty`] 같은 plugin 측 알림을 받기 위해 사용한다.
/// fd 수신 경로는 host에서 사용하지 않지만(현재 host는 fd를 보내기만 함), API 일관성을 위해
/// 동일한 reader 타입을 노출한다.
pub struct HandleStreamReader {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    carry: Vec<u8>,
    #[cfg(unix)]
    fd_queue: VecDeque<std::os::fd::RawFd>,
    #[cfg(windows)]
    inner: self::windows::PipeServerStream,
    #[cfg(windows)]
    carry: Vec<u8>,
}

impl HandleStreamReader {
    #[cfg(unix)]
    fn from_unix(stream: std::os::unix::net::UnixStream) -> Self {
        Self {
            inner: stream,
            carry: Vec::with_capacity(4096),
            fd_queue: VecDeque::new(),
        }
    }

    #[cfg(windows)]
    fn from_windows(stream: self::windows::PipeServerStream) -> Self {
        Self {
            inner: stream,
            carry: Vec::with_capacity(4096),
        }
    }

    /// 다음 메시지 한 건을 blocking으로 받는다. `HandleAttach`의 ancillary fd가 있으면
    /// 같이 반환한다. 연결이 닫히면 `UnexpectedEof` io 에러.
    #[cfg(unix)]
    pub fn recv_message(
        &mut self,
    ) -> io::Result<(HandleChannelMessage, Option<std::os::fd::RawFd>)> {
        loop {
            if let Some(nl) = self.carry.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = self.carry.drain(..=nl).collect();
                let line_str = std::str::from_utf8(&line_bytes[..nl])
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                    .trim();
                if line_str.is_empty() {
                    continue;
                }
                let msg: HandleChannelMessage = serde_json::from_str(line_str)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let aux_fd = match msg {
                    HandleChannelMessage::HandleAttach { .. } => self.fd_queue.pop_front(),
                    _ => None,
                };
                return Ok((msg, aux_fd));
            }

            let mut buf = [0u8; 4096];
            let (n, fds) = unix_wire::recv_with_fd(&self.inner, &mut buf)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "handle channel closed",
                ));
            }
            self.carry.extend_from_slice(&buf[..n]);
            for fd in fds {
                self.fd_queue.push_back(fd);
            }
        }
    }

    /// Windows: 파이프에서 NDJSON 라인을 파싱해 돌려준다. host 측은 plugin 이 보내는
    /// [`HandleChannelMessage::Dirty`] 만 받으므로 반환 핸들은 보통 `None`. `HandleAttach`
    /// 가 온다면(비정상) in-band `handle` 필드를 그대로 노출한다.
    #[cfg(windows)]
    pub fn recv_message(&mut self) -> io::Result<(HandleChannelMessage, Option<u64>)> {
        use std::io::Read;
        loop {
            if let Some(nl) = self.carry.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = self.carry.drain(..=nl).collect();
                let line_str = std::str::from_utf8(&line_bytes[..nl])
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                    .trim();
                if line_str.is_empty() {
                    continue;
                }
                let msg: HandleChannelMessage = serde_json::from_str(line_str)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let handle = match msg {
                    HandleChannelMessage::HandleAttach { handle, .. } => handle,
                    _ => None,
                };
                return Ok((msg, handle));
            }

            let mut buf = [0u8; 4096];
            let n = self.inner.read(&mut buf)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "handle channel closed",
                ));
            }
            self.carry.extend_from_slice(&buf[..n]);
        }
    }
}

/// 보조 채널 listener. 호스트 부팅 시 한 번만 bind한다.
///
/// accept 스레드 하나가 모든 incoming connection을 받고, plugin이 보낸 첫 줄의
/// `AuthMessage`로 토큰을 매칭한 뒤 [`HandleListener::expect_connection`]을 호출한
/// caller에게 stream을 분배한다.
///
/// Unix/Windows 양쪽 구현 완료. [`HandleListener::bind`]가 Unix는 `AF_UNIX` socket을,
/// Windows는 Named Pipe(overlapped accept 루프)를 연다.
/// 보조 핸들 채널의 handshake 대기 맵 poison 을 보고했는가(첫 1 회만).
///
/// [`crate::listener`] 의 메인 TCP handshake 맵과 같은 형태다 — 임계구역이 `HashMap`
/// insert/remove 뿐이라 패닉이 나도 불변식이 성립하므로 복구가 맞다. 조용히 버리면
/// 등록이 안 된 채 caller 의 `recv` 만 흘러 **plugin 이 왜 aux 채널을 못 여는지 timeout
/// 으로만 보이고**, 수락 쪽에서 버리면 이미 연결한 plugin 이 무음으로 거절된다.
static HANDLE_PENDING_POISONED: AtomicBool = AtomicBool::new(false);
const HANDLE_PENDING_WHAT: &str = "plugin aux handle channel pending map";

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
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::SystemTime;

        // (pid, nanos) 만으로는 같은 프로세스의 동시 bind(테스트 병렬 실행 등)가 동일
        // 나노초에 겹쳐 경로가 충돌할 수 있다 — 프로세스 전역 단조 시퀀스로 유일성 보장.
        // nanos 는 이전 프로세스가 남긴 stale 파일과의 충돌 회피용으로 유지한다.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);

        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let socket_path =
            std::env::temp_dir().join(format!("tasty-handle-{pid}-{:x}-{seq}.sock", nanos as u64));

        // stale 파일이 남아 있으면 unlink. 다음 bind를 위한 idempotent 정리.
        // NotFound는 정상 — 그 외 에러는 bind도 실패할 가능성이 높아 알린다.
        if let Err(e) = std::fs::remove_file(&socket_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "stale handle socket {} unlink failed: {e}",
                socket_path.display()
            );
        }

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

    /// 보조 채널을 bind. Windows 는 Named Pipe 서버 인스턴스를 만들고 accept 루프를
    /// 띄운다. Unix `bind` 와 동형이되, socket file 대신 파이프 이름(`\\.\pipe\...`)을
    /// endpoint 로 쓴다.
    #[cfg(windows)]
    pub fn bind() -> io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};

        // Unix 와 같은 이유(동일 프로세스 동시 bind 충돌 방지)로 프로세스 전역 단조 seq.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let name = self::windows::unique_pipe_name(seq);

        // accept thread 시작 전에 첫 인스턴스를 미리 만들어, 자식이 빠르게 connect 해도
        // 대기 인스턴스가 항상 존재하게 한다. FILE_FLAG_FIRST_PIPE_INSTANCE 로 이름 선점.
        let first = self::windows::create_pipe_instance(&name, true)?;
        let endpoint = name.clone();

        let pending: Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>> = Arc::default();
        let pending_clone = pending.clone();
        let accept_thread = std::thread::Builder::new()
            .name("plugin-handle-listener".to_string())
            .spawn(move || accept_loop_windows(name, first, pending_clone))?;

        Ok(Self {
            endpoint,
            pending,
            _accept_thread: accept_thread,
        })
    }

    /// plugin spawn에 전달할 endpoint 문자열. Unix는 socket path, Windows는 pipe 이름.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 해당 token으로 connect할 plugin의 stream을 기다린다. `timeout` 안에 안 오면 `None`.
    #[cfg(test)]
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
        tasty_utils::poison::recover_mutex(
            self.pending.lock(),
            HANDLE_PENDING_WHAT,
            &HANDLE_PENDING_POISONED,
        )
        .insert(token.to_string(), tx);
        rx
    }

    /// 미사용 mailbox 명시적 제거. expect_connection 의 timeout cleanup 경로.
    #[cfg(test)]
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
        // NotFound는 race(테스트가 직접 정리한 경우 등)에서 정상.
        if let Err(e) = std::fs::remove_file(&self._socket_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!(
                "handle socket {} drop unlink failed: {e}",
                self._socket_path.display()
            );
        }
    }
}

#[cfg(unix)]
fn handle_incoming_unix(
    stream: std::os::unix::net::UnixStream,
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>>,
) {
    let Some(auth) = read_auth_unix(&stream) else {
        return;
    };
    let tx_opt = tasty_utils::poison::recover_mutex(
        pending.lock(),
        HANDLE_PENDING_WHAT,
        &HANDLE_PENDING_POISONED,
    )
    .remove(&auth.token);
    match tx_opt {
        Some(tx) => accept_handshake_unix(stream, auth, tx),
        None => reject_handshake_unix(&stream, &auth),
    }
}

/// 인증 라인을 읽어 파싱 — 실패하면 warn 후 `None`(caller 는 그대로 drop).
/// 성공하면 read_timeout 을 해제한 상태로 반환(핸드셰이크 이후 정상 read 재개 대비).
#[cfg(unix)]
fn read_auth_unix(stream: &std::os::unix::net::UnixStream) -> Option<AuthMessage> {
    if let Err(e) = stream.set_read_timeout(Some(AUTH_READ_TIMEOUT)) {
        tracing::warn!("handle channel: set_read_timeout failed: {e}");
    }
    let auth = read_auth_message_unix(stream)?;
    if let Err(e) = stream.set_read_timeout(None) {
        tracing::warn!("handle channel: clearing read_timeout failed: {e}");
    }
    Some(auth)
}

/// stream 을 clone 해 한 줄 읽고 `AuthMessage` 로 파싱. 실패 사유는 내부에서 warn.
#[cfg(unix)]
fn read_auth_message_unix(stream: &std::os::unix::net::UnixStream) -> Option<AuthMessage> {
    let line = read_auth_line_unix(stream)?;
    match serde_json::from_str(line.trim()) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!("handle channel: invalid auth message: {e}");
            None
        }
    }
}

/// stream 을 clone 해 인증 라인 한 줄을 읽는다.
#[cfg(unix)]
fn read_auth_line_unix(stream: &std::os::unix::net::UnixStream) -> Option<String> {
    let cloned = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("handle channel: stream clone failed: {e}");
            return None;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line) {
        tracing::warn!("handle channel: auth read failed: {e}");
        return None;
    }
    Some(line)
}

/// 토큰 매칭 성공 — auth_ack(true) 송신 후 `HandleStream` 을 대기 중인 spawn 측에 handoff.
#[cfg(unix)]
fn accept_handshake_unix(
    stream: std::os::unix::net::UnixStream,
    auth: AuthMessage,
    tx: mpsc::Sender<HandleStream>,
) {
    if !send_auth_ack_unix_ok(&stream, &auth) {
        return;
    }
    tracing::debug!(
        "handle channel: plugin '{}' authenticated on aux channel",
        auth.plugin_id
    );
    if let Err(e) = tx.send(HandleStream::from_unix(stream)) {
        tracing::warn!(
            "handle channel: plugin '{}' handle stream handoff failed: {e}",
            auth.plugin_id
        );
    }
}

/// 성공 auth_ack 송신. 실패하면 warn 후 false(caller 는 그대로 drop).
#[cfg(unix)]
fn send_auth_ack_unix_ok(stream: &std::os::unix::net::UnixStream, auth: &AuthMessage) -> bool {
    if let Err(e) = send_auth_ack_unix(stream, true, None) {
        tracing::warn!(
            "handle channel: plugin '{}' auth_ack send failed: {e} — dropping",
            auth.plugin_id
        );
        return false;
    }
    true
}

/// 토큰 매칭 실패(unknown/expired) — 거부 ack 송신 후 drop.
#[cfg(unix)]
fn reject_handshake_unix(stream: &std::os::unix::net::UnixStream, auth: &AuthMessage) {
    tracing::warn!(
        "handle channel: auth with unknown/expired token (plugin_id={})",
        auth.plugin_id
    );
    if let Err(e) = send_auth_ack_unix(stream, false, Some("token mismatch")) {
        tracing::debug!("handle channel: auth_ack(false) send failed: {e}");
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
    let line =
        serde_json::to_string(&env).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut w = stream;
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Windows accept 루프. 한 인스턴스에서 클라이언트를 기다리고, 붙으면 다음 인스턴스를
/// 먼저 만든 뒤(연속 connect 손실 방지) 연결을 별도 스레드에서 인증 처리한다.
#[cfg(windows)]
fn accept_loop_windows(
    name: String,
    first: self::windows::PipeServerStream,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>>,
) {
    let mut current = first;
    loop {
        if let Err(e) = self::windows::accept(&current) {
            tracing::warn!("handle channel accept error: {e}");
            match self::windows::create_pipe_instance(&name, false) {
                Ok(next) => {
                    current = next;
                    continue;
                }
                Err(e2) => {
                    tracing::error!("handle channel: recreate pipe failed: {e2} — listener stops");
                    return;
                }
            }
        }
        // 다음 클라이언트용 인스턴스를 먼저 만들어 대기 인스턴스가 끊기지 않게 한다.
        let next = match self::windows::create_pipe_instance(&name, false) {
            Ok(n) => n,
            Err(e) => {
                tracing::error!("handle channel: next pipe instance failed: {e} — listener stops");
                handle_incoming_windows(current, &pending);
                return;
            }
        };
        let connected = std::mem::replace(&mut current, next);
        // auth 는 짧지만 느린 자식이 accept 스레드를 막지 않게 연결마다 스레드로 처리.
        let p = pending.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("plugin-handle-auth".to_string())
            .spawn(move || handle_incoming_windows(connected, &p))
        {
            tracing::warn!("handle channel: auth thread spawn failed: {e}");
        }
    }
}

/// Windows 연결 하나의 인증 핸드셰이크. Unix `handle_incoming_unix` 미러.
#[cfg(windows)]
fn handle_incoming_windows(
    mut stream: self::windows::PipeServerStream,
    pending: &Arc<Mutex<HashMap<String, mpsc::Sender<HandleStream>>>>,
) {
    let line = match read_line_windows(&mut stream) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("handle channel: auth read failed: {e}");
            return;
        }
    };
    let auth: AuthMessage = match serde_json::from_str(line.trim()) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("handle channel: invalid auth message: {e}");
            return;
        }
    };

    let tx_opt = tasty_utils::poison::recover_mutex(
        pending.lock(),
        HANDLE_PENDING_WHAT,
        &HANDLE_PENDING_POISONED,
    )
    .remove(&auth.token);

    match tx_opt {
        Some(tx) => {
            if let Err(e) = send_auth_ack_windows(&mut stream, true, None) {
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
            if let Err(e) = tx.send(HandleStream::from_windows(stream)) {
                tracing::warn!(
                    "handle channel: plugin '{}' handle stream handoff failed: {e}",
                    auth.plugin_id
                );
            }
        }
        None => {
            tracing::warn!(
                "handle channel: auth with unknown/expired token (plugin_id={})",
                auth.plugin_id
            );
            if let Err(e) = send_auth_ack_windows(&mut stream, false, Some("token mismatch")) {
                tracing::debug!("handle channel: auth_ack(false) send failed: {e}");
            }
        }
    }
}

/// 파이프에서 개행까지 한 줄을 읽는다. auth 라인 뒤 바이트를 over-read 하지 않도록
/// 1바이트씩 읽는다(handoff 후 reader 스레드가 이어질 Dirty 바이트를 잃지 않게).
#[cfg(windows)]
fn read_line_windows(stream: &mut self::windows::PipeServerStream) -> io::Result<String> {
    use std::io::Read;
    let mut buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            break; // EOF
        }
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(windows)]
fn send_auth_ack_windows(
    stream: &mut self::windows::PipeServerStream,
    ok: bool,
    reason: Option<&str>,
) -> io::Result<()> {
    let env = AuthAckEnvelope {
        auth_ack: AuthAck {
            ok,
            reason: reason.map(|s| s.to_string()),
        },
    };
    let line =
        serde_json::to_string(&env).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

#[cfg(unix)]
mod unix_wire {
    //! Unix `sendmsg`/`recvmsg` + `SCM_RIGHTS` 헬퍼.
    //!
    //! 보조 채널은 stream socket이지만, fd 전달이 필요한 메시지는 라인 바이트와
    //! ancillary control message를 같은 `sendmsg`로 묶어 보낸다. 수신측은 `recvmsg`로
    //! 둘을 한꺼번에 꺼낸다.
    use std::io;
    use std::mem;
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::UnixStream;

    /// `cmsghdr`의 alignment 요구사항을 만족하는 cmsg 버퍼.
    /// `cmsghdr`는 보통 long-aligned이므로 `u64` 백킹 배열로 8B 정렬 보장.
    #[repr(C)]
    union CmsgBuf {
        bytes: [u8; 64],
        _align: [u64; 8],
    }

    impl CmsgBuf {
        fn new() -> Self {
            Self { bytes: [0u8; 64] }
        }
        fn as_mut_ptr(&mut self) -> *mut u8 {
            // SAFETY: union의 두 필드는 같은 메모리를 공유. bytes 포인터는 항상 유효.
            unsafe { self.bytes.as_mut_ptr() }
        }
        fn len(&self) -> usize {
            64
        }
    }

    /// 한 fd만 담을 수 있는 cmsg 공간.
    fn cmsg_space_one_fd() -> u32 {
        // SAFETY: CMSG_SPACE는 입력에 대한 부수효과가 없는 macro/inline 함수.
        unsafe { libc::CMSG_SPACE(mem::size_of::<libc::c_int>() as u32) }
    }

    pub(super) fn send_with_fd(
        stream: &UnixStream,
        bytes: &[u8],
        aux_fd: Option<RawFd>,
    ) -> io::Result<()> {
        let iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut _,
            iov_len: bytes.len(),
        };
        // SAFETY: zero-initialized msghdr는 sendmsg에 안전한 초기 상태.
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_iov = &iov as *const _ as *mut _;
        msg.msg_iovlen = 1;

        // cmsg 버퍼 — fd 1개 분량, 8B alignment 보장.
        let mut cmsg_buf = CmsgBuf::new();
        if let Some(fd) = aux_fd {
            let cmsg_space = cmsg_space_one_fd() as usize;
            assert!(cmsg_buf.len() >= cmsg_space);
            msg.msg_control = cmsg_buf.as_mut_ptr() as *mut _;
            msg.msg_controllen = cmsg_space as _;

            // SAFETY: msg.msg_control이 유효한 64B 버퍼를 가리키고 msg.msg_controllen이 그 안에 들어감.
            // CMSG_FIRSTHDR / CMSG_DATA / write_unaligned 가 단일 cmsg 헤더 구성에 묶여
            // 하나의 atomic 한 작업이라 블록 분할이 불필요.
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            unsafe {
                let cmsg_ptr = libc::CMSG_FIRSTHDR(&msg);
                if cmsg_ptr.is_null() {
                    return Err(io::Error::other("CMSG_FIRSTHDR returned null"));
                }
                (*cmsg_ptr).cmsg_level = libc::SOL_SOCKET;
                (*cmsg_ptr).cmsg_type = libc::SCM_RIGHTS;
                (*cmsg_ptr).cmsg_len = libc::CMSG_LEN(mem::size_of::<libc::c_int>() as u32) as _;
                let data_ptr = libc::CMSG_DATA(cmsg_ptr) as *mut libc::c_int;
                std::ptr::write_unaligned(data_ptr, fd);
            }
        }

        let stream_fd = stream.as_raw_fd();
        // SAFETY: stream_fd는 open된 socket, msg는 위에서 모두 valid하게 채움.
        let n = unsafe { libc::sendmsg(stream_fd, &msg, 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        // 짧게 보냈으면 에러로 — 우리는 한 줄을 한 번에 보내야 SCM_RIGHTS와 묶임이 깨지지 않는다.
        if (n as usize) != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!(
                    "handle channel sendmsg short write: {} of {}",
                    n,
                    bytes.len()
                ),
            ));
        }
        Ok(())
    }

    /// 한 번의 `recvmsg`로 최대 `buf.len()` 바이트와 ancillary fd 목록을 받는다.
    /// 반환된 fds의 소유권은 caller에게 이전 — Drop 시 close하지 않으면 leak.
    pub(super) fn recv_with_fd(
        stream: &UnixStream,
        buf: &mut [u8],
    ) -> io::Result<(usize, Vec<RawFd>)> {
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut _,
            iov_len: buf.len(),
        };
        // SAFETY: zero-initialized msghdr는 recvmsg에 안전한 초기 상태.
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;

        // 여유 있게 fd 4개까지. 8B 정렬 보장.
        #[repr(C)]
        union RecvCmsgBuf {
            bytes: [u8; 256],
            _align: [u64; 32],
        }
        let mut cmsg_buf = RecvCmsgBuf { bytes: [0u8; 256] };
        // SAFETY: cmsg_buf의 bytes 필드 포인터.
        msg.msg_control = unsafe { cmsg_buf.bytes.as_mut_ptr() } as *mut _;
        msg.msg_controllen = 256 as _;

        let stream_fd = stream.as_raw_fd();
        // SAFETY: stream_fd는 open된 socket, msg는 위에서 valid하게 초기화.
        let n = unsafe { libc::recvmsg(stream_fd, &mut msg, 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut fds = Vec::new();
        // SAFETY: msg는 위에서 valid한 cmsg buffer를 가짐.
        let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !cmsg.is_null() {
            // SAFETY: cmsg는 non-null이고 msg가 가진 cmsg buffer 내부 포인터.
            let (level, ty, len) = unsafe {
                let h = &*cmsg;
                (h.cmsg_level, h.cmsg_type, h.cmsg_len)
            };
            if level == libc::SOL_SOCKET && ty == libc::SCM_RIGHTS {
                // SAFETY: CMSG_LEN(0)으로 헤더 크기 추출.
                let header_len = unsafe { libc::CMSG_LEN(0) } as usize;
                let data_len = (len as usize).saturating_sub(header_len);
                let n_fds = data_len / mem::size_of::<libc::c_int>();
                // SAFETY: CMSG_DATA는 cmsg 안의 data 시작 포인터.
                let data_ptr = unsafe { libc::CMSG_DATA(cmsg) } as *const libc::c_int;
                for i in 0..n_fds {
                    // SAFETY: data_ptr 부터 n_fds * sizeof(c_int) 범위 내. add 와
                    // read_unaligned 가 cmsg 단일 fd 읽기로 묶여있어 분할 시 가독성 저하.
                    #[allow(clippy::multiple_unsafe_ops_per_block)]
                    let fd = unsafe { std::ptr::read_unaligned(data_ptr.add(i)) };
                    fds.push(fd);
                }
            }
            // SAFETY: 동일 msg에 대한 다음 cmsg.
            cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
        }

        Ok((n as usize, fds))
    }
}

// Windows Named Pipe 구현은 02c에서 채워진다. 02b에서는 module이 빈 placeholder를
// 가지지만, type 참조가 컴파일되도록 stub 타입만 둔다.

#[cfg(windows)]
mod windows;

#[cfg(all(test, unix))]
#[path = "handle_channel/channel_tests.rs"]
mod channel_tests;

#[cfg(all(test, windows))]
#[path = "handle_channel/channel_tests_windows.rs"]
mod channel_tests_windows;
