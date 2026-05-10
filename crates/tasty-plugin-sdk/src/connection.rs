//! 호스트와의 양방향 NDJSON 채널.
//!
//! Plugin 부팅 흐름:
//! 1. `connect_and_authenticate(env)` — 호스트 포트에 TCP connect, 첫 줄에 [`AuthMessage`] 송신.
//! 2. [`Connection::send_event`]로 호스트에 알림 송신 (Hello, Log, IpcCall 등).
//! 3. [`Connection::recv`]로 호스트의 요청을 한 줄씩 수신.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::Result;

use tasty_plugin_protocol::{AuthMessage, PluginEvent, PluginRequest, PluginResponse};

use crate::env::PluginEnv;

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
    pub fn connect_and_authenticate(env: &PluginEnv) -> Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", env.host_port))?;
        // 짧은 read timeout으로 try_recv 패턴을 흉내낼 수 있게 한다.
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        let mut writer = stream.try_clone()?;
        let auth = AuthMessage {
            plugin_id: env.plugin_id.clone(),
            token: env.token.clone(),
        };
        let line = serde_json::to_string(&auth)?;
        writeln!(writer, "{line}")?;
        writer.flush()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
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
            Ok(0) => Err(anyhow::anyhow!("host closed connection")),
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
            Err(e) => Err(anyhow::anyhow!("recv error: {e}")),
        }
    }
}
