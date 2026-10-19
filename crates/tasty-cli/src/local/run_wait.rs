//! 접수 응답의 `run`으로 최종 결과를 기다리는 명령의 공통 흐름. 접수 응답을 한 줄 JSON으로
//! 출력하고, 결과 조회 메서드로 끝날 때까지 조회해 마지막 결과를 두 번째 줄로 출력한다.
//! 최종 상태가 `succeeded`가 아니거나 상한 안에 끝나지 않으면 종료 코드 1이다.
//! 접수 요청에는 일반 명령처럼 읽기 상한을 두지 않고, 후속 조회만 남은 대기 시간까지 읽는다.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};
use tasty_ipc::client::IpcConnection;
use tasty_ipc::protocol::JsonRpcRequest;

use crate::out::outln;

const POLL: Duration = Duration::from_millis(200);

/// `request`를 보내 접수 줄을 출력하고, `result_method`로 결과를 기다려 출력한다.
/// `limit`은 접수 응답으로 대기 상한을 정한다. run이 없는 접수(이전 호스트)는 접수 줄로 끝낸다.
pub(crate) fn dispatch_and_wait(
    port_file: Option<&str>,
    request: &JsonRpcRequest,
    result_method: &str,
    limit: impl FnOnce(&Value) -> Duration,
) -> Result<()> {
    let port = crate::port_file::read_port(port_file)?;
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
    let mut conn = IpcConnection::new(stream)?;
    let accepted = conn
        .send(request)
        .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
    outln!("{}", serde_json::to_string(&accepted).unwrap_or_default())?;
    let Some(run) = accepted.get("run").and_then(Value::as_u64) else {
        return Ok(());
    };
    let poll = poll_request(request, result_method, run);
    let value = wait_for_result(&mut conn, &poll, limit(&accepted))
        .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
    outln!("{}", serde_json::to_string(&value).unwrap_or_default())?;
    if !succeeded(&value) {
        std::process::exit(1);
    }
    Ok(())
}

/// `limit` 안에서 결과를 조회한다. 끝난 결과나 상한 시점의 마지막 진행 결과를 돌려준다.
/// 상한 안에 조회 응답이 없으면 `timed_out`을 단 진행 중 결과를 만들어 돌려준다.
fn wait_for_result(
    conn: &mut IpcConnection,
    poll: &JsonRpcRequest,
    limit: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let Some(value) = conn.send_reading_within(poll, left)? else {
            return Ok(timed_out(poll, limit));
        };
        let done = value.get("done").and_then(Value::as_bool) == Some(true);
        if done || Instant::now() >= deadline {
            return Ok(value);
        }
        std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

/// 조회 응답이 상한 안에 오지 않았을 때 마지막 줄로 내는 결과. 최종 결과는 아직 모른다.
fn timed_out(poll: &JsonRpcRequest, limit: Duration) -> Value {
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
fn poll_request(request: &JsonRpcRequest, method: &str, run: u64) -> JsonRpcRequest {
    let mut poll = request.clone();
    poll.method = method.into();
    poll.params = json!({ "run": run });
    poll.idempotency_key = None;
    poll
}

fn succeeded(result: &Value) -> bool {
    result.get("done").and_then(Value::as_bool) == Some(true)
        && result.get("state").and_then(Value::as_str) == Some("succeeded")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(token: Option<&str>, agent: Option<&str>, key: Option<&str>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "hook_handler.dispatch".into(),
            params: json!({ "id": "user/x" }),
            id: Some(json!(1)),
            session_token: token.map(str::to_string),
            caller_agent_id: agent.map(str::to_string),
            response_timeout_ms: None,
            idempotency_key: key.map(str::to_string),
        }
    }

    #[test]
    fn only_a_finished_success_exits_zero() {
        assert!(succeeded(&json!({"done": true, "state": "succeeded"})));
        assert!(!succeeded(&json!({"done": true, "state": "partial"})));
        assert!(!succeeded(&json!({"done": false, "state": "running"})));
    }

    /// 접수에는 답하고 결과 조회에는 답하지 않는 서버. 클라이언트가 끊으면 끝난다.
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
            line.clear();
            assert_eq!(reader.read_line(&mut line).unwrap_or(0), 0);
        });
        (TcpStream::connect(addr).unwrap(), server)
    }

    #[test]
    fn an_unanswered_result_query_stops_at_the_wait_limit() {
        let (stream, server) = silent_after_accept();
        let mut conn = IpcConnection::new(stream).unwrap();
        let request = request(None, None, None);
        let accepted = conn.send(&request).unwrap();
        assert_eq!(accepted["run"], 1);
        let started = Instant::now();
        let value = wait_for_result(
            &mut conn,
            &poll_request(&request, "hook_handler.result", 1),
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
        server.join().unwrap();
    }

    #[test]
    fn the_result_poll_keeps_the_caller_identity() {
        let request = request(Some("tok"), Some("agent-1"), Some("k"));
        let poll = poll_request(&request, "hook_handler.result", 7);
        assert_eq!(poll.method, "hook_handler.result");
        assert_eq!(poll.params, json!({ "run": 7 }));
        assert_eq!(poll.session_token.as_deref(), Some("tok"));
        assert_eq!(poll.caller_agent_id.as_deref(), Some("agent-1"));
        assert_eq!(poll.idempotency_key, None);
    }
}
