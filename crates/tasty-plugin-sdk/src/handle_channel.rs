//! SDK 측 보조 핸들 채널 클라이언트.
//!
//! 메인 채널([`crate::connection::Connection`])이 핸드셰이크를 끝낸 직후 본 모듈의
//! [`HandleClient::connect`]가 호출되어 보조 채널을 연다. plugin spawn 환경변수
//! `TASTY_PLUGIN_HANDLE_ENDPOINT`가 비어 있으면 보조 채널을 사용하지 않는다 (host가
//! 활성화하지 않은 경우).
//!
//! 02b에서 핸드셰이크만 검증됐고, 02c에서 [`HandleClientReader`](host가 보내는
//! `HandleAttach` + ancillary fd 수신)와 plugin → host `Dirty` 송신 경로가 추가됐다.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::time::Duration;

use tasty_plugin_protocol::{AuthAckEnvelope, AuthMessage, HandleChannelMessage};

use crate::env::PluginEnv;
use crate::error::{PluginError, Result};

const AUTH_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// 보조 채널 클라이언트. 메인 채널 인증 직후 plugin runtime이 한 번 만든다.
///
/// Windows에서는 02c까지 stub이다 — [`HandleClient::connect`]가 `Unsupported` 에러를
/// 반환한다. 호출자는 이 에러를 fatal로 취급하지 않고 warn 후 plugin 본 루프를 그대로
/// 진행해야 한다 (보조 채널은 *추가* 동작).
#[derive(Debug)]
pub struct HandleClient {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    _phantom: std::marker::PhantomData<()>,
}

impl HandleClient {
    /// `env.handle_endpoint`이 있어야 호출된다. 없으면 호출자가 미리 분기.
    pub fn connect(env: &PluginEnv) -> Result<Self> {
        let endpoint = env.handle_endpoint.as_deref().ok_or_else(|| {
            PluginError::EnvMissing("TASTY_PLUGIN_HANDLE_ENDPOINT")
        })?;
        Self::connect_to(endpoint, env)
    }

    #[cfg(unix)]
    fn connect_to(endpoint: &str, env: &PluginEnv) -> Result<Self> {
        use std::os::unix::net::UnixStream;

        let stream = UnixStream::connect(endpoint)?;
        // AuthAck 읽기 동안만 짧은 timeout.
        stream.set_read_timeout(Some(AUTH_ACK_TIMEOUT))?;

        let mut writer = stream.try_clone()?;
        let auth = AuthMessage {
            plugin_id: env.plugin_id.clone(),
            token: env.token.clone(),
        };
        let line = serde_json::to_string(&auth)?;
        writeln!(writer, "{line}")?;
        writer.flush()?;

        let cloned = stream.try_clone()?;
        let mut reader = BufReader::new(cloned);
        let mut ack_line = String::new();
        let read_result = reader.read_line(&mut ack_line);
        // ack 후 timeout 해제 (이후 read는 blocking이 아니라 호출자 정책에 따라).
        stream.set_read_timeout(None)?;
        match read_result {
            Ok(0) => Err(PluginError::HandshakeTimeout),
            Ok(_) => {
                let trim = ack_line.trim();
                if trim.is_empty() {
                    return Err(PluginError::HandshakeTimeout);
                }
                let env_msg: AuthAckEnvelope = serde_json::from_str(trim)?;
                if env_msg.auth_ack.ok {
                    Ok(Self { inner: stream })
                } else {
                    Err(PluginError::HandshakeRejected {
                        reason: env_msg.auth_ack.reason,
                    })
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                Err(PluginError::HandshakeTimeout)
            }
            Err(e) => Err(PluginError::Io(e)),
        }
    }

    #[cfg(windows)]
    fn connect_to(_endpoint: &str, _env: &PluginEnv) -> Result<Self> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel on Windows is not implemented yet (Step 02c)",
        )))
    }

    /// 한 줄을 NDJSON으로 송신.
    #[cfg(unix)]
    pub fn write_line(&mut self, line: &str) -> Result<()> {
        let mut buf = line.as_bytes().to_vec();
        buf.push(b'\n');
        self.inner.write_all(&buf)?;
        self.inner.flush()?;
        Ok(())
    }

    /// 한 줄을 NDJSON으로 송신. Windows stub.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub fn write_line(&mut self, _line: &str) -> Result<()> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel write not implemented on Windows yet",
        )))
    }

    /// blocking으로 한 줄 읽기. read timeout이 설정되어 있다면 그 이내에 못 받으면 io 에러.
    #[cfg(unix)]
    pub fn read_line(&mut self) -> Result<String> {
        let cloned = self.inner.try_clone()?;
        let mut reader = BufReader::new(cloned);
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(PluginError::HostClosed);
        }
        Ok(line)
    }

    /// blocking으로 한 줄 읽기. Windows stub.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub fn read_line(&mut self) -> Result<String> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel read not implemented on Windows yet",
        )))
    }

    /// read timeout 조정. ping/pong 등 짧은 동기 대기 시 호출.
    #[cfg(unix)]
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        self.inner.set_read_timeout(timeout)?;
        Ok(())
    }

    /// read timeout 조정. Windows stub.
    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub fn set_read_timeout(&self, _timeout: Option<Duration>) -> Result<()> {
        Ok(())
    }

    /// 메시지 단위 송신. fd가 필요한 메시지는 SDK가 보내지 않으므로 일반 라인 쓰기.
    #[cfg(unix)]
    pub fn send_message(&mut self, msg: &HandleChannelMessage) -> Result<()> {
        let line = serde_json::to_string(msg)?;
        self.write_line(&line)
    }

    #[cfg(windows)]
    #[allow(clippy::unused_self)]
    pub fn send_message(&mut self, _msg: &HandleChannelMessage) -> Result<()> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel send_message not implemented on Windows yet",
        )))
    }

    /// 수신 측 reader를 분리해서 반환. write 핸들은 self에 남는다.
    #[cfg(unix)]
    pub fn reader(&self) -> Result<HandleClientReader> {
        let cloned = self.inner.try_clone()?;
        Ok(HandleClientReader::from_unix(cloned))
    }

    #[cfg(windows)]
    pub fn reader(&self) -> Result<HandleClientReader> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel reader not implemented on Windows yet",
        )))
    }
}

/// 보조 채널 위의 NDJSON 메시지를 한 줄씩 받는다. host가 보내는
/// [`HandleChannelMessage::HandleAttach`]는 ancillary data(SCM_RIGHTS)로 fd가 함께
/// 도착하므로, 반환값에 `Option<RawFd>`가 포함된다.
pub struct HandleClientReader {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    carry: Vec<u8>,
    #[cfg(unix)]
    fd_queue: VecDeque<std::os::fd::RawFd>,
    #[cfg(windows)]
    _phantom: std::marker::PhantomData<()>,
}

impl HandleClientReader {
    #[cfg(unix)]
    fn from_unix(stream: std::os::unix::net::UnixStream) -> Self {
        Self {
            inner: stream,
            carry: Vec::with_capacity(4096),
            fd_queue: VecDeque::new(),
        }
    }

    /// 다음 한 건을 blocking으로 받는다. `HandleAttach`의 ancillary fd는 같이 반환.
    /// 연결이 닫히면 `HostClosed`.
    #[cfg(unix)]
    pub fn recv_message(
        &mut self,
    ) -> Result<(HandleChannelMessage, Option<std::os::fd::RawFd>)> {
        loop {
            if let Some(nl) = self.carry.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = self.carry.drain(..=nl).collect();
                let line_str = std::str::from_utf8(&line_bytes[..nl])
                    .map_err(|e| PluginError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        e,
                    )))?
                    .trim();
                if line_str.is_empty() {
                    continue;
                }
                let msg: HandleChannelMessage = serde_json::from_str(line_str)?;
                let aux_fd = match msg {
                    HandleChannelMessage::HandleAttach { .. } => self.fd_queue.pop_front(),
                    _ => None,
                };
                return Ok((msg, aux_fd));
            }

            let mut buf = [0u8; 4096];
            let (n, fds) = unix_wire::recv_with_fd(&self.inner, &mut buf)?;
            if n == 0 {
                return Err(PluginError::HostClosed);
            }
            self.carry.extend_from_slice(&buf[..n]);
            for fd in fds {
                self.fd_queue.push_back(fd);
            }
        }
    }

    #[cfg(windows)]
    pub fn recv_message(&mut self) -> Result<(HandleChannelMessage, Option<u64>)> {
        Err(PluginError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle channel recv_message not implemented on Windows yet",
        )))
    }
}

#[cfg(unix)]
mod unix_wire {
    //! SDK 측 `recvmsg` + `SCM_RIGHTS` 헬퍼. host 측 동일 헬퍼와 의도적으로 같은 동작.
    use std::io;
    use std::mem;
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::UnixStream;

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

        // 8B 정렬 보장된 cmsg 버퍼.
        #[repr(C)]
        union RecvCmsgBuf {
            bytes: [u8; 256],
            _align: [u64; 32],
        }
        // SAFETY: union 두 필드 같은 영역 공유.
        let mut cmsg_buf = unsafe { RecvCmsgBuf { bytes: [0u8; 256] } };
        // SAFETY: bytes 필드 포인터.
        msg.msg_control = unsafe { cmsg_buf.bytes.as_mut_ptr() } as *mut _;
        msg.msg_controllen = 256 as _;

        let fd = stream.as_raw_fd();
        // SAFETY: fd는 open된 socket, msg는 valid한 초기화.
        let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut fds = Vec::new();
        // SAFETY: msg가 valid한 cmsg buffer를 가짐.
        let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !cmsg.is_null() {
            // SAFETY: cmsg non-null, msg가 가진 cmsg 영역 내부 포인터.
            let (level, ty, len) = unsafe {
                let h = &*cmsg;
                (h.cmsg_level, h.cmsg_type, h.cmsg_len)
            };
            if level == libc::SOL_SOCKET && ty == libc::SCM_RIGHTS {
                // SAFETY: header 크기 계산.
                let header_len = unsafe { libc::CMSG_LEN(0) } as usize;
                let data_len = (len as usize).saturating_sub(header_len);
                let n_fds = data_len / mem::size_of::<libc::c_int>();
                // SAFETY: CMSG_DATA는 cmsg 안의 data 시작 포인터.
                let data_ptr = unsafe { libc::CMSG_DATA(cmsg) } as *const libc::c_int;
                for i in 0..n_fds {
                    // SAFETY: data_ptr부터 n_fds * sizeof(c_int) 범위.
                    let f = unsafe { std::ptr::read_unaligned(data_ptr.add(i)) };
                    fds.push(f);
                }
            }
            // SAFETY: 동일 msg에 대한 다음 cmsg.
            cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
        }

        Ok((n as usize, fds))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn env_for(endpoint: &str) -> PluginEnv {
        PluginEnv {
            plugin_id: "com.test.plugin".into(),
            host_port: 0,
            token: "test-token".into(),
            host_api_version: "1".into(),
            plugin_dir: None,
            data_dir: None,
            config_path: None,
            log_path: None,
            handle_endpoint: Some(endpoint.to_string()),
        }
    }

    fn unique_socket_path(suffix: &str) -> std::path::PathBuf {
        // macOS의 SUN_LEN(~104B) 제한 — 짧게 유지. /tmp는 모든 unix에 존재.
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::path::PathBuf::from(format!(
            "/tmp/tst-h-{}-{:x}-{}.sock",
            pid,
            (nanos as u64) & 0xFFFF_FFFF,
            suffix
        ))
    }

    #[test]
    fn connect_succeeds_when_host_acks_ok() {
        let path = unique_socket_path("ok");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let path_clone = path.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let cloned = stream.try_clone().unwrap();
            let mut reader = BufReader::new(cloned);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let _ = writeln!(stream, "{{\"auth_ack\":{{\"ok\":true}}}}");
            let _ = stream.flush();
            thread::sleep(Duration::from_millis(200));
            drop(stream);
            // cleanup
            let _ = std::fs::remove_file(&path_clone);
        });
        let env = env_for(path.to_str().unwrap());
        let client = HandleClient::connect(&env).expect("auth ok");
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn connect_returns_rejected_when_host_acks_false() {
        let path = unique_socket_path("reject");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let path_clone = path.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let cloned = stream.try_clone().unwrap();
            let mut reader = BufReader::new(cloned);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let _ = writeln!(
                stream,
                "{{\"auth_ack\":{{\"ok\":false,\"reason\":\"nope\"}}}}"
            );
            let _ = stream.flush();
            thread::sleep(Duration::from_millis(50));
            let _ = std::fs::remove_file(&path_clone);
        });
        let env = env_for(path.to_str().unwrap());
        let err = HandleClient::connect(&env).expect_err("should be rejected");
        match err {
            PluginError::HandshakeRejected { reason } => {
                assert_eq!(reason.as_deref(), Some("nope"));
            }
            other => panic!("expected HandshakeRejected, got {other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn connect_returns_env_missing_when_endpoint_absent() {
        let mut env = env_for("/dummy");
        env.handle_endpoint = None;
        let err = HandleClient::connect(&env).expect_err("no endpoint");
        assert!(matches!(err, PluginError::EnvMissing(_)));
    }

    /// host측 unix_wire 헬퍼와 시그니처가 같은 sendmsg 보조. 테스트 외에는 SDK가 fd를
    /// 보낼 일이 없으므로 production에는 두지 않는다.
    fn test_sendmsg_with_fd(
        stream: &std::os::unix::net::UnixStream,
        bytes: &[u8],
        fd: std::os::fd::RawFd,
    ) -> std::io::Result<()> {
        use std::mem;
        use std::os::fd::AsRawFd;
        let iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut _,
            iov_len: bytes.len(),
        };
        // SAFETY: zero-init msghdr는 sendmsg에 valid한 초기 상태.
        let mut msg: libc::msghdr = unsafe { mem::zeroed() };
        msg.msg_iov = &iov as *const _ as *mut _;
        msg.msg_iovlen = 1;
        // 8B 정렬 보장된 cmsg 버퍼.
        #[repr(C)]
        union SendCmsgBuf {
            bytes: [u8; 64],
            _align: [u64; 8],
        }
        // SAFETY: union 두 필드 같은 영역.
        let mut cmsg_buf = unsafe { SendCmsgBuf { bytes: [0u8; 64] } };
        // SAFETY: CMSG_SPACE는 부수효과 없음.
        let cmsg_space = unsafe { libc::CMSG_SPACE(mem::size_of::<libc::c_int>() as u32) }
            as usize;
        // SAFETY: bytes 필드 포인터.
        msg.msg_control = unsafe { cmsg_buf.bytes.as_mut_ptr() } as *mut _;
        msg.msg_controllen = cmsg_space as _;
        // SAFETY: cmsg_buf에 64B 여유. cmsg 헤더 채우기.
        unsafe {
            let cmsg_ptr = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg_ptr).cmsg_level = libc::SOL_SOCKET;
            (*cmsg_ptr).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg_ptr).cmsg_len =
                libc::CMSG_LEN(mem::size_of::<libc::c_int>() as u32) as _;
            let data_ptr = libc::CMSG_DATA(cmsg_ptr) as *mut libc::c_int;
            std::ptr::write_unaligned(data_ptr, fd);
        }
        let sfd = stream.as_raw_fd();
        // SAFETY: sfd 유효, msg 유효.
        let n = unsafe { libc::sendmsg(sfd, &msg, 0) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[test]
    fn reader_decodes_handle_attach_with_fd() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        use tasty_plugin_protocol::SharedBufferId;

        // socketpair로 양방향 stream을 만들고, 한쪽을 host처럼 sendmsg, 다른쪽을 SDK reader처럼 처리.
        let (host_side, sdk_side) = UnixStream::pair().expect("socketpair");

        // 보낼 fd로 /dev/null 열기.
        let dev_null = std::fs::File::open("/dev/null").expect("open /dev/null");
        let send_fd = dev_null.as_raw_fd();

        let msg = HandleChannelMessage::HandleAttach {
            request_id: 7,
            id: SharedBufferId(99),
            size: 8192,
        };
        let mut line = serde_json::to_string(&msg).unwrap();
        line.push('\n');
        test_sendmsg_with_fd(&host_side, line.as_bytes(), send_fd)
            .expect("sendmsg with fd");

        // SDK reader는 보통 HandleClient::reader()로 만들어지지만, 테스트에서는 직접 구성.
        let mut reader = HandleClientReader::from_unix(sdk_side);
        let (got_msg, got_fd) = reader.recv_message().expect("recv");
        assert_eq!(got_msg, msg);
        let fd = got_fd.expect("expected fd on HandleAttach");
        // 받은 fd는 원래 fd와 *다른* 번호여야 한다 (kernel이 dup).
        assert_ne!(fd, send_fd, "expected dup'd fd");
        // 받은 fd로 /dev/null이 실제로 매핑됐는지 확인 — read는 빈 buffer를 반환.
        // SAFETY: fd는 방금 dup된 valid한 file descriptor.
        let n = unsafe {
            let mut buf = [0u8; 1];
            libc::read(fd, buf.as_mut_ptr() as *mut _, 1)
        };
        assert_eq!(n, 0, "/dev/null read returns 0 (EOF)");
        // 받은 fd close — leak 방지.
        // SAFETY: fd는 우리가 위에서 받은 valid한 file descriptor.
        unsafe { libc::close(fd) };
    }

    #[test]
    fn reader_decodes_plain_dirty_without_fd() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use tasty_plugin_protocol::{Rect, SharedBufferId};

        let (mut host_side, sdk_side) = UnixStream::pair().expect("socketpair");
        let msg = HandleChannelMessage::Dirty {
            id: SharedBufferId(3),
            rect: Some(Rect { x: 1, y: 2, w: 3, h: 4 }),
        };
        let line = serde_json::to_string(&msg).unwrap() + "\n";
        host_side.write_all(line.as_bytes()).unwrap();

        let mut reader = HandleClientReader::from_unix(sdk_side);
        let (got, fd) = reader.recv_message().expect("recv");
        assert_eq!(got, msg);
        assert!(fd.is_none(), "Dirty 메시지는 fd 없음");
    }
}
