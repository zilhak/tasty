//! `plugin enable` 응답은 기동을 시작했다는 뜻일 뿐이다. 응답의 기동 세대로 `plugin.show` 의
//! `start` 를 다시 읽어 연결 성공·실패·취소가 날 때까지 기다린다. 연결 대기 상한(10초)보다
//! 조금 길게 기다리고, 그래도 결과가 없으면 아직 시작 중이라고 알린다.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_ipc::client::IpcConnection;
use tasty_ipc::protocol::JsonRpcRequest;

use crate::Commands;
use crate::commands::PluginCommands;

const WAIT_LIMIT: Duration = Duration::from_secs(15);
/// 이 시간이 지나도 시작 중이면 진행 중이라는 줄을 표준 오류에 한 번 쓴다. 표준 출력은 최종 결과만 싣는다.
const PROGRESS_AFTER: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 응답의 `start` 를 최종 결과로 바꾼다. 실패·취소·시간 초과면 알릴 문장을 돌려준다.
/// 기동 세대를 싣지 않는 이전 호스트의 응답은 그대로 둔다.
pub(crate) fn settle(
    command: &Commands,
    request: &JsonRpcRequest,
    conn: &mut IpcConnection,
    value: &mut Value,
) -> Option<String> {
    let Commands::Plugin {
        command: PluginCommands::Enable { id },
    } = command
    else {
        return None;
    };
    let generation = value.pointer("/start/generation")?.as_u64()?;
    let started = Instant::now();
    let deadline = started + WAIT_LIMIT;
    let mut progress_shown = false;
    while state_of(value) == Some("starting") && Instant::now() < deadline {
        if !progress_shown && started.elapsed() >= PROGRESS_AFTER {
            progress_shown = true;
            crate::out::errln!("{}", progress_message(id, generation));
        }
        std::thread::sleep(POLL_INTERVAL);
        let show = JsonRpcRequest {
            caller_agent_id: request.caller_agent_id.clone(),
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".into(),
            method: "plugin.show".into(),
            params: json!({ "id": id }),
            id: Some(Value::from(2)),
            session_token: request.session_token.clone(),
        };
        let shown = match conn.send(&show) {
            Ok(v) => v,
            Err(e) => return Some(e.to_string()),
        };
        let Some(start) = shown.get("start").filter(|s| !s.is_null()) else {
            break;
        };
        if start.get("generation").and_then(Value::as_u64) != Some(generation) {
            // 다른 요청이 새로 기동했다. 이 요청의 기동은 결과 전에 대체됐다.
            value["start"]["state"] = Value::from("cancelled");
            break;
        }
        value["start"] = start.clone();
    }
    outcome_message(id, value)
}

fn progress_message(id: &str, generation: u64) -> String {
    tasty_i18n::t_args(
        "cli.plugin.enable_waiting",
        &[
            id,
            &generation.to_string(),
            &WAIT_LIMIT.as_secs().to_string(),
            id,
        ],
    )
}

fn state_of(value: &Value) -> Option<&str> {
    value.pointer("/start/state").and_then(Value::as_str)
}

fn outcome_message(id: &str, value: &Value) -> Option<String> {
    match state_of(value)? {
        "connected" => None,
        "failed" => {
            let kind = value
                .pointer("/start/failure/kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let message = value
                .pointer("/start/failure/message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(tasty_i18n::t_args(
                "cli.plugin.enable_failed",
                &[id, kind, message, id],
            ))
        }
        "cancelled" => Some(tasty_i18n::t_fmt("cli.plugin.enable_cancelled", id)),
        _ => Some(tasty_i18n::t_args(
            "cli.plugin.enable_still_starting",
            &[id, &WAIT_LIMIT.as_secs().to_string(), id],
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_final_state_gets_its_message() {
        tasty_i18n::init("en");
        let failed = json!({"start": {"generation": 2, "state": "failed",
            "failure": {"kind": "connect_failed", "message": "plugin did not connect", "generation": 2}}});
        let text = outcome_message("com.example.p", &failed).expect("failure");
        assert!(
            text.contains("connect_failed") && text.contains("plugin did not connect"),
            "{text}"
        );
        assert!(outcome_message("p", &json!({"start": {"state": "connected"}})).is_none());
        assert!(outcome_message("p", &json!({"start": {"state": "cancelled"}})).is_some());
        let slow = outcome_message("p", &json!({"start": {"state": "starting"}})).expect("timeout");
        assert!(slow.contains("15"), "{slow}");
        assert!(outcome_message("p", &json!({"enabled": "p"})).is_none());
        let waiting = progress_message("com.example.p", 7);
        assert!(
            waiting.contains("com.example.p") && waiting.contains('7'),
            "{waiting}"
        );
    }
}
