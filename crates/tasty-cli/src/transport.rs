use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;

use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A reusable IPC connection that keeps a single BufReader across multiple requests.
pub struct IpcConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl IpcConnection {
    /// 읽기 타임아웃은 걸지 않는다 — 프로토콜에 **의도적으로 무한 대기하는 호출**이
    /// 있기 때문이다(`tasty agent task-await --timeout-ms 0`, 사용자 응답을 기다리는
    /// approval 등). 한 값으로 자르면 그 호출들이 정상 동작 중에 끊긴다. 응답 없는
    /// 상대를 감지하는 몫은 [`IpcConnection::send`] 의 EOF 판정이 진다.
    pub fn new(stream: TcpStream) -> Result<Self> {
        // Nagle 해제 — 요청 한 줄을 보내고 응답을 기다리는 순수 request-response 라
        // 지연시켜 합칠 뒷 데이터가 애초에 없다. Nagle 이 켜져 있으면 요청 줄이 두
        // 세그먼트로 쪼개지는 순간(`writeln!` 은 본문과 개행을 나눠 쓴다) 뒷조각이
        // 상대의 delayed ACK(~40ms)까지 붙잡혀 **모든 CLI 명령**에 그만큼이 얹힌다.
        // 실패해도 연결 자체는 유효하므로 에러로 올리지 않는다.
        if let Err(e) = stream.set_nodelay(true) {
            tracing::warn!("IPC 연결: TCP_NODELAY 설정 실패(지연 증가 가능): {e}");
        }
        let writer = stream.try_clone()?;
        let reader = BufReader::new(stream);
        Ok(Self { writer, reader })
    }

    /// Send a JSON-RPC request and read the response.
    pub fn send(&mut self, request: &JsonRpcRequest) -> Result<serde_json::Value> {
        let json = serde_json::to_string(request)?;
        // 개행까지 한 버퍼에 담아 **한 번의 write** 로 보낸다 — `writeln!` 은 본문과
        // 개행을 각각 write 해 세그먼트를 쪼갠다(위 `TCP_NODELAY` 주석 참고).
        let mut line = json.into_bytes();
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;

        loop {
            let mut line = String::new();
            // EOF(`read_line` 이 0 을 반환)를 반드시 먼저 걸러낸다. EOF 는 이후로
            // 영원히 0 을 반환하므로, 빈 줄 skip 으로 흘려보내면 이 루프가 코어
            // 하나를 100% 로 태우며 무한 스핀한다(커널 블록이 아니라 유저스페이스
            // 스핀이라 겉보기 hang 과 구분도 어렵다). 호스트가 종료 중이거나
            // 크래시/SIGKILL 로 죽으면 실제로 밟는 경로다.
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                anyhow::bail!(
                    "tasty instance closed the connection without responding \
                     (host may be shutting down)"
                );
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let response: JsonRpcResponse = serde_json::from_str(trimmed)?;

            if let Some(error) = response.error {
                anyhow::bail!("Error ({}): {}", error.code, error.message);
            }

            return Ok(response.result.unwrap_or(serde_json::Value::Null));
        }
    }
}
