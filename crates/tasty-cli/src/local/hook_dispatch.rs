//! `hook-handler dispatch`: 접수 응답을 한 줄 JSON으로 출력한 뒤 `hook_handler.result`로 그 run의
//! 최종 결과를 기다려 두 번째 줄로 출력한다. 최종 상태가 `succeeded`가 아니거나 기다리는 동안
//! 끝나지 않으면 종료 코드 1이다. 마지막 줄만 읽으면 최종 결과를 얻는다.

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
        let mut conn = IpcConnection::new(stream)?;
        let accepted = conn
            .send(&request)
            .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
        outln!("{}", serde_json::to_string(&accepted).unwrap_or_default())?;
        // run이 없는 이전 호스트는 접수 응답만 준다.
        let Some(run) = accepted.get("run").and_then(Value::as_u64) else {
            return Ok(());
        };
        let deadline = Instant::now() + wait_limit(&accepted);
        let mut poll = request.clone();
        poll.method = "hook_handler.result".into();
        poll.params = json!({ "run": run });
        poll.idempotency_key = None;
        loop {
            let value = conn
                .send(&poll)
                .unwrap_or_else(|e| crate::rpc_error::exit_with(&e));
            let done = value.get("done").and_then(Value::as_bool) == Some(true);
            if done || Instant::now() >= deadline {
                outln!("{}", serde_json::to_string(&value).unwrap_or_default())?;
                if !succeeded(&value) {
                    std::process::exit(1);
                }
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }
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
}
