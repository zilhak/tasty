//! stream.open으로 JSON-RPC 소켓을 길이 기반 바이너리 프레임 연결로 전환한다.

use std::io::{BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Result, bail};

use crate::protocol::JsonRpcRequest;
use crate::stream::{self, StreamAck, StreamFrame, StreamTag};

/// A streaming-channel connection to a running tasty instance.
pub struct StreamConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl StreamConnection {
    /// Upgrade a connected TCP socket into a streaming channel.
    ///
    /// Sends the `stream.open` handshake line, then blocks for the server's
    /// `Control` ack frame. Returns the connection and the server-assigned
    /// client id.
    pub fn open(stream: TcpStream, proto: u32) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, None, None, stream::HEARTBEAT_TIMEOUT)
    }

    /// surface를 attach한다. handshake ack 다음의 attached/attach_error Control을 별도로 읽어야 한다.
    pub fn open_attach(stream: TcpStream, proto: u32, target: u32) -> Result<(Self, u32)> {
        Self::open_with(
            stream,
            proto,
            Some(target),
            None,
            None,
            stream::HEARTBEAT_TIMEOUT,
        )
    }

    /// workspace 전체를 attach한다. Data payload는 surface ID로 시작하는 mux 형식이다.
    pub fn open_attach_workspace(
        stream: TcpStream,
        proto: u32,
        workspace: u32,
    ) -> Result<(Self, u32)> {
        Self::open_attach_workspace_with_timeout(
            stream,
            proto,
            workspace,
            stream::HEARTBEAT_TIMEOUT,
        )
    }

    /// [`Self::open_attach_workspace`]와 같고 무응답 제한으로 `timeout`을 쓴다. handshake ack 대기부터 적용된다.
    pub fn open_attach_workspace_with_timeout(
        stream: TcpStream,
        proto: u32,
        workspace: u32,
        timeout: Duration,
    ) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, Some(workspace), None, timeout)
    }

    /// workspace holder와 별개의 bulk 연결을 연다. Data는 PTY 입력 대신 파일 청크다.
    /// 대상 workspace에 활성 holder가 있어야 서버가 전송을 허용한다.
    /// 대화형 attach가 파일 전송을 기다리지 않도록 같은 SSH 터널에서 별도 소켓을 쓴다.
    pub fn open_bulk(stream: TcpStream, proto: u32, workspace: u32) -> Result<(Self, u32)> {
        Self::open_bulk_with_timeout(stream, proto, workspace, stream::HEARTBEAT_TIMEOUT)
    }

    /// [`Self::open_bulk`]와 같고 무응답 제한으로 `timeout`을 쓴다. handshake ack 대기부터 적용된다.
    pub fn open_bulk_with_timeout(
        stream: TcpStream,
        proto: u32,
        workspace: u32,
        timeout: Duration,
    ) -> Result<(Self, u32)> {
        Self::open_with(stream, proto, None, None, Some(workspace), timeout)
    }

    fn open_with(
        stream: TcpStream,
        proto: u32,
        target: Option<u32>,
        target_workspace: Option<u32>,
        bulk_workspace: Option<u32>,
        timeout: Duration,
    ) -> Result<(Self, u32)> {
        // 받은 소켓을 자식 프로세스가 물려받지 않게 가장 먼저 끈다(Windows). 복제는 아래에서 끈다.
        tasty_utils::socket_inherit::disable_inherit(&stream)?;
        // 조용한 네트워크 단절 감지용 read timeout. writer/reader 는 이 소켓의 clone이라
        // 옵션이 공유되므로 여기서 한 번만 걸면 이후 모든 `recv()`(핸드셰이크 ack 대기
        // 포함)에 적용된다.
        stream.set_read_timeout(Some(timeout))?;
        // 작은 프레임의 분할 전송이 delayed ACK를 기다리지 않도록 Nagle을 끈다.
        if let Err(e) = stream.set_nodelay(true) {
            tracing::warn!("attach stream: TCP_NODELAY 설정 실패(지연 증가 가능): {e}");
        }
        let writer = tasty_utils::socket_inherit::try_clone_uninherited(&stream)?;
        let mut reader = BufReader::new(stream);
        let mut writer = writer;

        let mut params = serde_json::json!({ "proto": proto });
        if let Some(t) = target {
            params["target"] = serde_json::json!(t);
        }
        if let Some(w) = target_workspace {
            params["target_workspace"] = serde_json::json!(w);
        }
        if let Some(w) = bulk_workspace {
            params["bulk_workspace"] = serde_json::json!(w);
        }
        let req = JsonRpcRequest {
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: stream::STREAM_OPEN_METHOD.to_string(),
            params,
            id: Some(serde_json::json!(1)),
            session_token: None,
        };
        writeln!(writer, "{}", serde_json::to_string(&req)?)?;
        writer.flush()?;

        let ack_frame = stream::read_frame(&mut reader)?;
        if ack_frame.tag != StreamTag::Control {
            bail!("expected Control ack frame, got {:?}", ack_frame.tag);
        }
        let ack: StreamAck = serde_json::from_slice(&ack_frame.payload)?;
        if !ack.ok {
            bail!(
                "stream.open rejected: {}",
                ack.error.unwrap_or_else(|| "unknown error".to_string())
            );
        }

        Ok((Self { writer, reader }, ack.client_id.unwrap_or(0)))
    }

    /// Clone the writer half of the socket so input can be sent from one thread
    /// while another blocks reading frames (mirror-dump / raw bridge).
    pub fn try_clone_writer(&self) -> Result<TcpStream> {
        Ok(tasty_utils::socket_inherit::try_clone_uninherited(
            &self.writer,
        )?)
    }

    /// Write one frame to the server.
    pub fn send(&mut self, tag: StreamTag, payload: &[u8]) -> Result<()> {
        stream::write_frame(&mut self.writer, tag, payload)?;
        Ok(())
    }

    /// Block for the next frame from the server.
    pub fn recv(&mut self) -> Result<StreamFrame> {
        Ok(stream::read_frame(&mut self.reader)?)
    }

    /// Signal a graceful close to the server.
    pub fn detach(&mut self) -> Result<()> {
        stream::write_frame(&mut self.writer, StreamTag::Detach, &[])?;
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::StreamConnection;
    use crate::stream::{self, StreamTag};
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use tasty_utils::socket_inherit::{is_inheritable, make_inheritable};

    /// 연결의 reader·writer와 꺼낸 writer 복제는 자식 프로세스가 물려받지 않는다. 넘기는 소켓의 상속을
    /// 일부러 켜서, open이 끄는 호출이 빠지면 std가 돌려준 값과 상관없이 실패하게 한다.
    #[test]
    fn stream_connection_sockets_are_not_inheritable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(socket.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            let ack = serde_json::json!({"ok": true, "proto": 1, "client_id": 7});
            stream::write_frame(
                &mut socket,
                StreamTag::Control,
                &serde_json::to_vec(&ack).expect("ack"),
            )
            .expect("write ack");
            socket
        });
        let socket = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        make_inheritable(&socket).expect("make inheritable");
        let (conn, _) = StreamConnection::open(socket, 1).expect("open");
        let _server_socket = server.join().expect("server");
        let writer_clone = conn.try_clone_writer().expect("writer clone");
        let flags: Vec<(&str, bool)> = [
            ("reader", conn.reader.get_ref()),
            ("writer", &conn.writer),
            ("writer clone", &writer_clone),
        ]
        .into_iter()
        .map(|(name, socket)| (name, is_inheritable(socket).expect(name)))
        .collect();
        assert!(
            flags.iter().all(|(_, inheritable)| !inheritable),
            "inheritable handles remain: {flags:?}"
        );
    }
}
