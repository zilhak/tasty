//! Plugin CLI 명령 중 호스트 IPC를 거치지 않는 local-only 처리.

use std::io::{Read, Seek, SeekFrom};
use std::net::TcpStream;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::cli::transport::IpcConnection;
use crate::ipc::server::IpcServer;

fn log_dir() -> Result<PathBuf> {
    tasty_core::paths::tasty_home()
        .map(|d| d.join("plugins-logs"))
        .ok_or_else(|| anyhow::anyhow!("could not determine tasty home directory"))
}

/// Polls `plugin.audit_follow` every `interval_ms` and prints new records as
/// they arrive. Runs until Ctrl-C (no graceful shutdown — process exit).
pub fn run_audit_follow(
    caller_kind: Option<&str>,
    caller_id: Option<&str>,
    method_prefix: Option<&str>,
    decision: Option<&str>,
    batch: u64,
    interval_ms: u64,
) -> Result<()> {
    let port = IpcServer::read_port_file()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;
    let mut conn = IpcConnection::new(stream)?;

    let mut base = Map::new();
    if let Some(v) = caller_kind {
        base.insert("caller_kind".into(), json!(v));
    }
    if let Some(v) = caller_id {
        base.insert("caller_id".into(), json!(v));
    }
    if let Some(v) = method_prefix {
        base.insert("method_prefix".into(), json!(v));
    }
    if let Some(v) = decision {
        base.insert("decision".into(), json!(v));
    }
    base.insert("limit".into(), json!(batch));

    let session_token = std::env::var("TASTY_SESSION_TOKEN").ok();
    let mut next_id: i64 = 1;
    let mut after_ts: Option<u64> = None;
    let mut after_seq: Option<u64> = None;
    loop {
        let mut params = base.clone();
        if let Some(t) = after_ts {
            params.insert("after_ts_ms".into(), json!(t));
        }
        if let Some(s) = after_seq {
            params.insert("after_seq".into(), json!(s));
        }
        let req = crate::ipc::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "plugin.audit_follow".to_string(),
            params: Value::Object(params),
            id: Some(json!(next_id)),
            session_token: session_token.clone(),
        };
        next_id += 1;
        let resp = conn.send(&req)?;
        if let Some(ts) = resp.get("next_after_ts_ms").and_then(|v| v.as_u64()) {
            after_ts = Some(ts);
        }
        if let Some(seq) = resp.get("next_after_seq").and_then(|v| v.as_u64()) {
            after_seq = Some(seq);
        }
        if let Some(records) = resp.get("records").and_then(|v| v.as_array()) {
            for rec in records {
                println!("{}", serde_json::to_string(rec).unwrap_or_default());
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
}

pub fn run_plugin_logs(plugin_id: &str, follow: bool) -> Result<()> {
    let path = log_dir()?.join(format!("{plugin_id}.log"));
    if !path.exists() {
        anyhow::bail!(
            "no log file for plugin '{}' at {}",
            plugin_id,
            path.display()
        );
    }
    if !follow {
        let s = std::fs::read_to_string(&path)?;
        print!("{s}");
        return Ok(());
    }
    let mut file = std::fs::File::open(&path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    print!("{buf}");
    let mut pos = file.metadata()?.len();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        file.seek(SeekFrom::Start(pos))?;
        let mut chunk = String::new();
        let n = file.read_to_string(&mut chunk)? as u64;
        if n > 0 {
            print!("{chunk}");
            pos += n;
        }
    }
}
