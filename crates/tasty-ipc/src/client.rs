//! 클라이언트 측 IPC 연결 — 실행 중인 tasty 인스턴스에 붙는 쪽.
//!
//! 서버 측([`crate::server`] / [`crate::session`])과 wire 프레이밍
//! ([`crate::protocol`] / [`crate::stream`])이 이 크레이트에 있으므로,
//! 그 짝인 클라이언트 연결도 같은 곳에 둔다. CLI 와 본체 GUI 가 함께 쓴다.
//!
//! - [`IpcConnection`] — BufReader 를 유지하는 JSON-RPC request-response 연결
//! - [`StreamConnection`] — attach/bulk 스트림 연결

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// 호스트가 JSON-RPC 오류로 답했다 — **코드를 데이터로 들고 있는** 실패.
///
/// 이전에는 `anyhow::bail!("Error ({code}): {message}")` 로 곧장 문자열이 됐고, 그래서
/// 호출자가 코드를 쓰려면 자기가 만든 문장을 자기가 다시 파싱해야 했다. `message` 는
/// 답한 쪽(호스트 또는 plugin)이 만든 산문이라 **로케일을 탈 수 있는** 반면 `code` 는
/// 프로토콜 값이라 안 탄다 — 그 둘을 갈라 두는 것이 이 타입의 전부다.
///
/// `Display` 는 종전 문자열 그대로다. 기존 호출자의 출력은 한 글자도 바뀌지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRpcCallError {
    pub code: i32,
    pub message: String,
}

impl std::fmt::Display for JsonRpcCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error ({}): {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcCallError {}

pub mod stream;

pub use stream::StreamConnection;

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
                return Err(JsonRpcCallError {
                    code: error.code,
                    message: error.message,
                }
                .into());
            }

            return Ok(response.result.unwrap_or(serde_json::Value::Null));
        }
    }
}
