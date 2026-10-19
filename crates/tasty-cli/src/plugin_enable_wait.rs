//! `plugin enable` 응답은 기동을 시작했다는 뜻일 뿐이다. 응답의 기동 세대로 `plugin.show` 의
//! `start` 를 다시 읽어 연결 성공·실패·취소가 날 때까지 기다린다. 연결 대기 상한(10초)보다
//! 조금 길게 기다리고, 그래도 결과가 없으면 아직 시작 중이라고 알린다.
//!
//! 첫 `plugin.enable` 요청은 일반 명령처럼 보낸다. 후속 조회만 남은 대기 시간 안에서 읽는다
//! ([`IpcConnection::send_reading_within`]: 응답 한 줄을 읽는 전체가 남은 시간 안이다). 상한에 걸린
//! 연결은 다시 쓰지 않으므로 진행 안내 시점에 걸리면 새로 연결해 이어 조회하고, 그 연결 수립도
//! 남은 시간으로 제한한다. 서버가 답하지 않거나 응답을 조금씩 보내도 진행 안내와 상한이 지켜진다.

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

/// 이 명령이 결과를 기다리는가. 기다리는 명령은 루프라 `--response-timeout-ms` 를 받지 않는다.
pub(crate) fn waits(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Plugin {
            command: PluginCommands::Enable { no_wait: false, .. }
        }
    )
}

/// 응답의 `start` 를 최종 결과로 바꾼다. 실패·취소·시간 초과면 알릴 문장을 돌려준다.
/// 기동 세대를 싣지 않는 이전 호스트의 응답은 그대로 둔다.
pub(crate) fn settle(
    command: &Commands,
    request: &JsonRpcRequest,
    conn: IpcConnection,
    reconnect: impl FnMut(Duration) -> anyhow::Result<IpcConnection>,
    value: &mut Value,
) -> Option<String> {
    let Commands::Plugin {
        command: PluginCommands::Enable { id, no_wait: false },
    } = command
    else {
        return None;
    };
    let generation = value.pointer("/start/generation")?.as_u64()?;
    let show = show_request(request, id);
    let waited = wait(
        conn,
        reconnect,
        &show,
        generation,
        value,
        WAIT_LIMIT,
        PROGRESS_AFTER,
        |g| {
            crate::out::errln!("{}", progress_message(id, g));
        },
    );
    if let Err(e) = waited {
        return Some(e.to_string());
    }
    outcome_message(id, value)
}

/// 후속 조회 요청. 원 요청의 세션 토큰·에이전트 ID를 싣고, 멱등 키와 응답 시한은 원 요청에만 둔다.
fn show_request(request: &JsonRpcRequest, id: &str) -> JsonRpcRequest {
    let mut show = request.clone();
    show.method = "plugin.show".into();
    show.params = json!({ "id": id });
    show.id = Some(Value::from(2));
    show.idempotency_key = None;
    show.response_timeout_ms = None;
    show
}

#[allow(clippy::too_many_arguments)] // 이유: 시험이 상한·안내 시점·안내 출력을 바꿔 넣는다.
fn wait(
    conn: IpcConnection,
    mut reconnect: impl FnMut(Duration) -> anyhow::Result<IpcConnection>,
    show: &JsonRpcRequest,
    generation: u64,
    value: &mut Value,
    limit: Duration,
    progress_after: Duration,
    mut progress: impl FnMut(u64),
) -> anyhow::Result<()> {
    let started = Instant::now();
    let deadline = started + limit;
    let progress_at = started + progress_after;
    let mut progress_shown = false;
    let mut conn = Some(conn);
    while state_of(value) == Some("starting") {
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        if !progress_shown && now >= progress_at {
            progress_shown = true;
            progress(generation);
        }
        // 진행 안내 전이면 그 시점까지만 읽어, 답이 없어도 안내를 늦추지 않는다.
        let until = if progress_shown {
            deadline
        } else {
            progress_at
        };
        let left = until.saturating_duration_since(now);
        let mut connection = match conn.take() {
            Some(connection) => connection,
            None => match reconnect(left) {
                Ok(connection) => connection,
                // 남은 시간 안에 연결하지 못했다. 안내 시점이나 상한에서 다시 판단한다.
                Err(_) if Instant::now() >= until => continue,
                Err(e) => return Err(e),
            },
        };
        let Some(shown) = connection.send_reading_within(show, left)? else {
            continue;
        };
        conn = Some(connection);
        let Some(start) = shown.get("start").filter(|s| !s.is_null()) else {
            return Ok(());
        };
        if start.get("generation").and_then(Value::as_u64) != Some(generation) {
            // 다른 요청이 새로 기동했다. 이 요청의 기동은 결과 전에 대체됐다.
            value["start"]["state"] = Value::from("cancelled");
            return Ok(());
        }
        value["start"] = start.clone();
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
    Ok(())
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
    use std::io::{BufRead, Write};
    use std::net::{TcpListener, TcpStream};

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

    #[test]
    fn only_a_waiting_enable_refuses_the_response_bound() {
        let enable = |no_wait| Commands::Plugin {
            command: PluginCommands::Enable {
                id: "p".into(),
                no_wait,
            },
        };
        assert!(waits(&enable(false)));
        assert!(!waits(&enable(true)));
    }

    #[test]
    fn the_show_query_keeps_the_caller_but_not_the_first_requests_bound() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "plugin.enable".into(),
            params: json!({ "id": "p" }),
            id: Some(json!(1)),
            session_token: Some("tok".into()),
            caller_agent_id: Some("agent-1".into()),
            response_timeout_ms: Some(500),
            idempotency_key: Some("k".into()),
        };
        let show = show_request(&request, "p");
        assert_eq!(show.method, "plugin.show");
        assert_eq!(show.session_token.as_deref(), Some("tok"));
        assert_eq!(show.caller_agent_id.as_deref(), Some("agent-1"));
        assert_eq!(
            (show.response_timeout_ms, show.idempotency_key),
            (None, None)
        );
    }

    /// 요청을 받기만 하고 답하지 않는 서버. 연결마다 받아 둔다.
    fn silent_server() -> (u16, std::thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut held = Vec::new();
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until {
                if let Ok((s, _)) = listener.accept() {
                    s.set_nonblocking(false).unwrap();
                    let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                    let mut line = String::new();
                    // 요청 한 줄을 읽고 답하지 않는다.
                    let _read = r.read_line(&mut line);
                    held.push(s);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            held.len()
        });
        (port, server)
    }

    /// 서버가 후속 조회에 답하지 않아도 진행 안내를 제때 내고 상한에서 멈춘다.
    #[test]
    fn a_silent_server_still_gets_the_progress_line_and_the_limit() {
        let (port, server) = silent_server();
        let connect = move |_left| -> anyhow::Result<IpcConnection> {
            IpcConnection::new(TcpStream::connect(("127.0.0.1", port))?)
        };
        let mut value = json!({"start": {"generation": 3, "state": "starting"}});
        let show = show_request(
            &JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "plugin.enable".into(),
                params: json!({}),
                id: Some(json!(1)),
                session_token: None,
                caller_agent_id: None,
                response_timeout_ms: None,
                idempotency_key: None,
            },
            "p",
        );
        let started = Instant::now();
        let mut progress_at = None;
        wait(
            connect(Duration::ZERO).unwrap(),
            connect,
            &show,
            3,
            &mut value,
            Duration::from_millis(900),
            Duration::from_millis(300),
            |g| {
                assert_eq!(g, 3);
                progress_at = Some(started.elapsed());
            },
        )
        .unwrap();
        let waited = started.elapsed();
        let shown = progress_at.expect("progress line");
        assert!(
            shown >= Duration::from_millis(300) && shown < Duration::from_millis(700),
            "{shown:?}"
        );
        assert!(
            waited >= Duration::from_millis(900) && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        assert_eq!(state_of(&value), Some("starting"));
        assert!(
            server.join().unwrap() >= 2,
            "a fresh connection after the progress point"
        );
    }

    /// 다른 요청이 새 세대로 기동하면 이 요청은 취소로 끝난다.
    #[test]
    fn a_newer_generation_cancels_this_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = std::io::BufReader::new(s.try_clone().unwrap());
            let mut w = s;
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            writeln!(
                w,
                r#"{{"jsonrpc":"2.0","id":2,"result":{{"start":{{"generation":4,"state":"starting"}}}}}}"#
            )
            .unwrap();
        });
        let conn = IpcConnection::new(TcpStream::connect(("127.0.0.1", port)).unwrap()).unwrap();
        let mut value = json!({"start": {"generation": 3, "state": "starting"}});
        let show = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "plugin.show".into(),
            params: json!({"id": "p"}),
            id: Some(json!(2)),
            session_token: None,
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        wait(
            conn,
            |_| anyhow::bail!("no reconnect"),
            &show,
            3,
            &mut value,
            Duration::from_secs(2),
            Duration::from_secs(1),
            |_| {},
        )
        .unwrap();
        assert_eq!(state_of(&value), Some("cancelled"));
        server.join().unwrap();
    }
}
