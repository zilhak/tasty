//! `hook-handler dispatch`: 접수 응답을 한 줄 JSON으로 출력한 뒤 `hook_handler.result`로 그 run의
//! 최종 결과를 기다려 두 번째 줄로 출력한다. 최종 상태가 `succeeded`가 아니거나 기다리는 동안
//! 끝나지 않으면 종료 코드 1이다. 조회 응답이 없어도 대기 상한에서 멈춘다. 마지막 줄만 읽으면
//! 최종 결과를 얻는다.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};
use tasty_ipc::client::IpcConnection;

use crate::Commands;
use crate::dispatch::{ClientCommand, ClientCtx};
use crate::out::outln;

const POLL: Duration = Duration::from_millis(200);
/// 호스트의 스텝 응답 대기 상한과 같다. 시퀀스는 스텝 수만큼 기다린다.
const STEP_WAIT: Duration = Duration::from_secs(10);

pub struct HookDispatch<'a> {
    pub command: &'a Commands,
}

impl ClientCommand for HookDispatch<'_> {
    fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
        let request = crate::request::command_to_request(self.command);
        let port = crate::port_file::read_port(ctx.port_file)?;
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
            anyhow::anyhow!(
                "{}",
                tasty_i18n::t_fmt2(
                    "cli.request.connect_failed",
                    &port.to_string(),
                    &e.to_string()
                )
            )
        })?;
        let socket = stream.try_clone()?;
        let mut conn = IpcConnection::new(stream)?;
        let accepted = conn
            .send(&request)
            .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
        outln!("{}", serde_json::to_string(&accepted).unwrap_or_default())?;
        // run이 없는 이전 호스트는 접수 응답만 준다.
        let Some(run) = accepted.get("run").and_then(Value::as_u64) else {
            return Ok(());
        };
        let poll = poll_request(&request, run);
        let value = wait_for_result(&mut conn, &socket, &poll, wait_limit(&accepted))
            .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
        outln!("{}", serde_json::to_string(&value).unwrap_or_default())?;
        if !succeeded(&value) {
            std::process::exit(1);
        }
        Ok(())
    }
}

/// `limit` 안에서 결과를 조회한다. 끝난 결과나 상한 시점의 마지막 진행 결과를 돌려준다.
/// 조회 응답도 남은 시간까지만 읽는다. 상한 안에 응답이 없으면 `timed_out`을 단 진행 중 결과를
/// 만들어 돌려준다. 그 뒤의 응답은 읽지 않으므로 연결을 다시 쓰지 않는다.
fn wait_for_result(
    conn: &mut IpcConnection,
    socket: &TcpStream,
    poll: &tasty_ipc::protocol::JsonRpcRequest,
    limit: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        // 0은 무기한 대기라 남은 시간이 없어도 최소 1ms로 둔다.
        socket.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
        let value = match conn.send(poll) {
            Ok(value) => value,
            Err(e) if is_read_timeout(&e) => return Ok(timed_out(poll, limit)),
            Err(e) => return Err(e),
        };
        let done = value.get("done").and_then(Value::as_bool) == Some(true);
        if done || Instant::now() >= deadline {
            return Ok(value);
        }
        std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn is_read_timeout(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>().is_some_and(|io| {
        matches!(
            io.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )
    })
}

/// 조회 응답이 상한 안에 오지 않았을 때 마지막 줄로 내는 결과. 최종 결과는 아직 모른다.
fn timed_out(poll: &tasty_ipc::protocol::JsonRpcRequest, limit: Duration) -> Value {
    let run = poll.params.get("run").cloned().unwrap_or(Value::Null);
    json!({
        "run": run,
        "done": false,
        "state": "running",
        "timed_out": true,
        "result": format!(
            "no answer to {} within {} ms; the final result is unknown",
            poll.method,
            limit.as_millis()
        ),
    })
}

/// 결과 조회 요청. 원 요청의 세션 토큰·에이전트 ID를 그대로 싣고, 멱등 키는 원 요청에만 둔다.
fn poll_request(
    request: &tasty_ipc::protocol::JsonRpcRequest,
    run: u64,
) -> tasty_ipc::protocol::JsonRpcRequest {
    let mut poll = request.clone();
    poll.method = "hook_handler.result".into();
    poll.params = json!({ "run": run });
    poll.idempotency_key = None;
    poll
}

/// 셸 핸들러는 프로세스 시작만 기다리고, 시퀀스는 스텝마다 응답 대기 상한을 더한다.
fn wait_limit(accepted: &Value) -> Duration {
    let steps = accepted.get("steps").and_then(Value::as_u64).unwrap_or(0) as u32;
    STEP_WAIT * (steps + 1)
}

fn succeeded(result: &Value) -> bool {
    result.get("done").and_then(Value::as_bool) == Some(true)
        && result.get("state").and_then(Value::as_str) == Some("succeeded")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_finished_success_exits_zero() {
        assert!(succeeded(&json!({"done": true, "state": "succeeded"})));
        assert!(!succeeded(&json!({"done": true, "state": "partial"})));
        assert!(!succeeded(&json!({"done": false, "state": "running"})));
        assert_eq!(wait_limit(&json!({"steps": 2})), Duration::from_secs(30));
        assert_eq!(wait_limit(&json!({})), Duration::from_secs(10));
    }

    /// 접수에는 답하고 결과 조회에는 답하지 않는 서버. 연결은 시험이 끝날 때까지 열어 둔다.
    fn silent_after_accept() -> (TcpStream, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            writeln!(
                writer,
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"accepted":true,"run":1,"action":"shell_command"}}}}"#
            )
            .unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            // 조회를 받고 답하지 않는다. 클라이언트가 끊으면 끝난다.
            line.clear();
            assert_eq!(reader.read_line(&mut line).unwrap_or(0), 0);
        });
        (TcpStream::connect(addr).unwrap(), server)
    }

    #[test]
    fn an_unanswered_result_query_stops_at_the_wait_limit() {
        let (stream, server) = silent_after_accept();
        let socket = stream.try_clone().unwrap();
        let mut conn = IpcConnection::new(stream).unwrap();
        let request = tasty_ipc::protocol::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "hook_handler.dispatch".into(),
            params: json!({ "id": "user/x" }),
            id: Some(json!(1)),
            session_token: None,
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let accepted = conn.send(&request).unwrap();
        assert_eq!(accepted["run"], 1);
        let started = Instant::now();
        let value = wait_for_result(
            &mut conn,
            &socket,
            &poll_request(&request, 1),
            Duration::from_millis(300),
        )
        .unwrap();
        let waited = started.elapsed();
        assert!(waited < Duration::from_secs(3), "waited {waited:?}");
        assert_eq!(value["timed_out"], true);
        assert_eq!(value["state"], "running");
        assert!(!succeeded(&value));
        assert_eq!(
            value["result"],
            "no answer to hook_handler.result within 300 ms; the final result is unknown"
        );
        drop(conn);
        drop(socket);
        server.join().unwrap();
    }

    #[test]
    fn the_result_poll_keeps_the_caller_identity() {
        let request = tasty_ipc::protocol::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "hook_handler.dispatch".into(),
            params: json!({ "id": "user/x" }),
            id: Some(json!(1)),
            session_token: Some("tok".into()),
            caller_agent_id: Some("agent-1".into()),
            response_timeout_ms: None,
            idempotency_key: Some("k".into()),
        };
        let poll = poll_request(&request, 7);
        assert_eq!(poll.method, "hook_handler.result");
        assert_eq!(poll.params, json!({ "run": 7 }));
        assert_eq!(poll.session_token.as_deref(), Some("tok"));
        assert_eq!(poll.caller_agent_id.as_deref(), Some("agent-1"));
        assert_eq!(poll.idempotency_key, None);
    }
}
