//! 호스트 측 보조 핸들 채널.
//!
//! 메인 TCP 채널([`crate::listener::HostListener`])은 fd/HANDLE을 운반할 수
//! 없으므로, 보조 채널을 별도로 둔다. Unix는 `AF_UNIX` socket, Windows는 Named Pipe.
//!
//! 02b에서 인증 핸드셰이크 + 채널 분배만 구현됐고, 02c에서 [`HandleStream::send_handle`]
//! (SCM_RIGHTS / DuplicateHandle)과 [`HandleStreamReader`](dirty 메시지 수신)가 추가됐다.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use tasty_plugin_protocol::{AuthAck, AuthAckEnvelope, HandleChannelMessage};

use crate::protocol::AuthMessage;

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
    inner: platform::PipeServerStream,
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

    /// Windows 측 stub. 02c-Windows에서 구현 예정.
    #[cfg(windows)]
    pub fn send_handle(&mut self, _msg: &HandleChannelMessage, _handle: u64) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel send_handle not implemented on Windows yet",
        ))
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

    #[cfg(windows)]
    pub fn reader(&self) -> io::Result<HandleStreamReader> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel reader not implemented on Windows yet",
        ))
    }
}

#[cfg(unix)]
impl HandleStream {
    fn from_unix(stream: std::os::unix::net::UnixStream) -> Self {
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
    _phantom: std::marker::PhantomData<()>,
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

    #[cfg(windows)]
    pub fn recv_message(&mut self) -> io::Result<(HandleChannelMessage, Option<u64>)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel recv_message not implemented on Windows yet",
        ))
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
        let socket_path =
            std::env::temp_dir().join(format!("tasty-handle-{pid}-{:x}.sock", nanos as u64));

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
        if let Ok(mut p) = self.pending.lock() {
            p.insert(token.to_string(), tx);
        }
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
    if let Err(e) = stream.set_read_timeout(Some(AUTH_READ_TIMEOUT)) {
        tracing::warn!("handle channel: set_read_timeout failed: {e}");
    }
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
    if let Err(e) = stream.set_read_timeout(None) {
        tracing::warn!("handle channel: clearing read_timeout failed: {e}");
    }

    let tx_opt = pending.lock().ok().and_then(|mut p| p.remove(&auth.token));

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
            if let Err(e) = tx.send(HandleStream::from_unix(stream)) {
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
    let line =
        serde_json::to_string(&env).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut w = stream;
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
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
