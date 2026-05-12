//! 호스트와의 양방향 NDJSON 채널.
//!
//! Plugin 부팅 흐름:
//! 1. [`Connection::connect`] — 호스트 포트에 TCP connect (인증 전 상태).
//! 2. [`Connection::authenticate`] — [`AuthMessage`] 송신 후 호스트의 `AuthAck`
//!    응답을 5초 안에 받으면 본 루프 진입. 거부 시 [`PluginError::HandshakeRejected`],
//!    무응답 시 [`PluginError::HandshakeTimeout`].
//! 3. [`Connection::send_event`]로 호스트에 알림 송신 (Hello, Log, IpcCall 등).
//! 4. [`Connection::try_recv`]로 호스트의 요청을 한 줄씩 수신.
//!
//! 편의 메서드 [`Connection::connect_and_authenticate`]는 1+2를 합친 것이며
//! 기존 호출자 호환을 위해 남아 있다.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use tasty_plugin_protocol::{
    protocol::AuthAckEnvelope, AuthMessage, PluginEvent, PluginRequest, PluginResponse,
};

use crate::env::PluginEnv;
use crate::error::{PluginError, Result};

/// 호스트가 AuthAck를 보낼 때까지 기다리는 최대 시간.
/// 호스트 측 `AUTH_READ_TIMEOUT`(5s)과 동일하게 맞춰 둔다.
pub(crate) const AUTH_ACK_TIMEOUT: Duration = Duration::from_secs(5);
/// 정상 메시지 루프에서의 read timeout. `try_recv` 패턴을 흉내내기 위해 짧게.
const RUN_READ_TIMEOUT: Duration = Duration::from_millis(50);

pub struct Connection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

/// 호스트가 plugin에 보낼 수 있는 메시지.
#[derive(Debug, Clone)]
pub enum HostMessage {
    Request(PluginRequest),
}

impl Connection {
    /// 호스트 포트에 TCP connect만 한다. AuthMessage는 아직 송신하지 않는다.
    /// 후속으로 [`Connection::authenticate`]를 반드시 호출해야 핸드셰이크가 완료된다.
    pub fn connect(env: &PluginEnv) -> Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", env.host_port)).map_err(|source| {
            PluginError::Connect {
                port: env.host_port,
                source,
            }
        })?;
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }

    /// AuthMessage를 송신하고 호스트의 AuthAck 한 줄을 [`AUTH_ACK_TIMEOUT`] 안에
    /// 수신한다. ok=false면 [`PluginError::HandshakeRejected`], 시간 안에 안 오면
    /// [`PluginError::HandshakeTimeout`].
    ///
    /// 성공하면 read timeout을 짧은 값(50ms)으로 복구해 try_recv 패턴이 동작한다.
    pub fn authenticate(&mut self, env: &PluginEnv) -> Result<()> {
        let auth = AuthMessage {
            plugin_id: env.plugin_id.clone(),
            token: env.token.clone(),
        };
        let line = serde_json::to_string(&auth)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        // AuthAck 한 줄 수신. 호스트가 silent drop했던 과거 버그를 막기 위함.
        self.writer.set_read_timeout(Some(AUTH_ACK_TIMEOUT))?;
        // reader 쪽도 같은 stream이므로 read_timeout이 공유된다.
        let mut ack_line = String::new();
        let read_result = self.reader.read_line(&mut ack_line);
        // 정상 루프 timeout으로 복구.
        self.writer.set_read_timeout(Some(RUN_READ_TIMEOUT))?;
        match read_result {
            Ok(0) => Err(PluginError::HandshakeTimeout),
            Ok(_) => {
                let trim = ack_line.trim();
                if trim.is_empty() {
                    return Err(PluginError::HandshakeTimeout);
                }
                let env_msg: AuthAckEnvelope = serde_json::from_str(trim)?;
                if env_msg.auth_ack.ok {
                    Ok(())
                } else {
                    Err(PluginError::HandshakeRejected {
                        reason: env_msg.auth_ack.reason,
                    })
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Err(PluginError::HandshakeTimeout)
            }
            Err(e) => Err(PluginError::Io(e)),
        }
    }

    /// [`connect`](Self::connect) + [`authenticate`](Self::authenticate) 조합.
    pub fn connect_and_authenticate(env: &PluginEnv) -> Result<Self> {
        let mut conn = Self::connect(env)?;
        conn.authenticate(env)?;
        Ok(conn)
    }

    /// 내부 stream/reader 분리. runtime이 Arc<Mutex<TcpStream>>로 감싸기 위해 사용.
    pub fn into_parts(self) -> (TcpStream, BufReader<TcpStream>) {
        (self.writer, self.reader)
    }

    /// `{"event": <PluginEvent>}` 형태로 호스트에 알림 송신.
    pub fn send_event(&mut self, event: &PluginEvent) -> Result<()> {
        let payload = serde_json::json!({ "event": event });
        let line = serde_json::to_string(&payload)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        Ok(())
    }

    /// 호스트의 요청에 대한 응답. id는 원 요청의 id를 그대로 echo.
    pub fn send_response(&mut self, response: &PluginResponse) -> Result<()> {
        let line = serde_json::to_string(response)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        Ok(())
    }

    /// 호스트로부터 한 줄 수신. timeout이면 `Ok(None)`.
    pub fn try_recv(&mut self) -> Result<Option<HostMessage>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(PluginError::HostClosed),
            Ok(_) => {
                let trim = line.trim();
                if trim.is_empty() {
                    return Ok(None);
                }
                let req: PluginRequest = serde_json::from_str(trim)?;
                Ok(Some(HostMessage::Request(req)))
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(PluginError::Io(e)),
        }
    }
}
