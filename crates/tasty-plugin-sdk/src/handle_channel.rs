//! SDK 측 보조 핸들 채널 클라이언트.
//!
//! 메인 채널([`crate::connection::Connection`])이 핸드셰이크를 끝낸 직후 본 모듈의
//! [`HandleClient::connect`]가 호출되어 보조 채널을 연다. plugin spawn 환경변수
//! `TASTY_PLUGIN_HANDLE_ENDPOINT`가 비어 있으면 보조 채널을 사용하지 않는다 (host가
//! 활성화하지 않은 경우).
//!
//! 02b 단계에서는 인증 + ping/pong만 검증된다. 02c에서 SCM_RIGHTS/HANDLE 수신 경로가
//! 본 module의 stream 위에 얹힌다.

use std::io::{self, BufRead, BufReader, Write};
use std::time::Duration;

use tasty_plugin_protocol::{AuthAckEnvelope, AuthMessage};

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
}
