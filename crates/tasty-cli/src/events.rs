//! `tasty events follow` — 위치를 들고 long-poll 을 반복한다.
//!
//! **커서는 이쪽이 든다.** 매 요청이 직전 답의 `next_offset` 을 싣고, 호스트는
//! 소비자별 상태를 두지 않는다. 그래서 끊겼다 붙어도 같은 자리에서 이어진다.
//!
//! 모양의 선례는 `plugin audit-follow` 다. 다른 점 하나 — 그쪽은 간격을 두고 다시
//! 묻고, 이쪽은 **호스트가 기다려 준다**(`wait_ms`). 그래서 새 사건이 없는 동안
//! 왕복이 안 생기고, 생겼을 때의 지연이 폴링 간격에 안 묶인다.

use std::net::TcpStream;

use anyhow::Result;
use serde_json::{Map, Value, json};

use tasty_ipc::client::IpcConnection;

use crate::out::outln;

/// 위치를 들고 long-poll 을 반복한다. Ctrl-C 까지 돈다.
pub fn run_follow(
    offset: u64,
    filter: Option<&str>,
    batch: u64,
    wait_ms: u64,
    port_file: Option<&str>,
) -> Result<()> {
    let port = crate::port_file::read_port(port_file)?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
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

    let session_token = std::env::var("TASTY_SESSION_TOKEN").ok();
    let mut next_id: i64 = 1;
    let mut cursor = offset;
    // 세대 표지. 첫 답이 정하고, 그 뒤 달라지면 호스트가 다시 선 것이다.
    let mut epoch: Option<u64> = None;
    loop {
        let mut params = Map::new();
        params.insert("offset".into(), json!(cursor));
        params.insert("max".into(), json!(batch));
        params.insert("wait_ms".into(), json!(wait_ms));
        if let Some(f) = filter {
            params.insert("filter".into(), json!(f));
        }
        let req = tasty_ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            jsonrpc: "2.0".to_string(),
            method: "events.fetch".to_string(),
            params: Value::Object(params),
            id: Some(json!(next_id)),
            session_token: session_token.clone(),
        };
        next_id += 1;
        let resp = conn.send(&req)?;

        if let Some(got) = resp.get("epoch").and_then(|v| v.as_u64()) {
            match epoch {
                None => epoch = Some(got),
                Some(had) if had != got => {
                    // 위치는 세대 안에서만 뜻이 있다. 새 세대에서 옛 위치를 계속 쓰면
                    // 엉뚱한 사건을 가리키므로 처음부터 다시 잡는다.
                    eprintln!("{}", tasty_i18n::t("cli.events.epoch_changed"));
                    epoch = Some(got);
                    cursor = 0;
                    continue;
                }
                Some(_) => {}
            }
        }
        // **조용히 넘어가지 않는다.** 건너뛴 수를 stderr 로 알린다 — stdout 은
        // `while read` 가 먹는 자리라 사건 줄만 간다.
        if resp
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let skipped = resp.get("skipped").and_then(|v| v.as_u64()).unwrap_or(0);
            eprintln!(
                "{}",
                tasty_i18n::t_fmt("cli.events.skipped", &skipped.to_string())
            );
        }
        if let Some(events) = resp.get("events").and_then(|v| v.as_array()) {
            for ev in events {
                outln!("{}", serde_json::to_string(ev).unwrap_or_default())?;
            }
            crate::out::flush()?;
        }
        if let Some(next) = resp.get("next_offset").and_then(|v| v.as_u64()) {
            cursor = next;
        }
    }
}
