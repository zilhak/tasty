//! Plugin CLI 명령 중 호스트 IPC를 거치지 않는 local-only 처리.

use std::io::{Read, Seek, SeekFrom};
use std::net::TcpStream;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::cli::transport::IpcConnection;
use crate::file::format::config::DetectorRuleDecl;
use crate::file::handler::config::PluginHandlerActionDecl;
use crate::ipc::port_file;
use crate::plugin::manifest::Manifest;

fn log_dir() -> Result<PathBuf> {
    tasty_utils::path::tasty_home()
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
    let port = port_file::read_port_file()?;
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

/// `tasty plugin doctor <id>` — Plugin manifest 의 contributes.detector / handler 를
/// 점검해 현재 호스트가 이해하지 못하는 rule kind (= `DetectorRuleDecl::Unknown`) 를
/// 표시한다. 호스트가 실행 중이지 않아도 작동하는 local-only 명령.
pub fn run_plugin_doctor(plugin_id: &str) -> Result<()> {
    let root = crate::plugin::plugin_root()
        .ok_or_else(|| anyhow::anyhow!("could not determine plugin root directory"))?;
    let plugin_dir = root.join(plugin_id);
    if !plugin_dir.join("tasty-plugin.toml").exists() {
        anyhow::bail!(
            "plugin '{}' not installed (no manifest at {})",
            plugin_id,
            plugin_dir.join("tasty-plugin.toml").display()
        );
    }
    let manifest = Manifest::load(&plugin_dir)
        .map_err(|e| anyhow::anyhow!("failed to load manifest for '{}': {e}", plugin_id))?;

    println!("Plugin: {}", manifest.id);
    println!("  name:             {}", manifest.name);
    println!("  version:          {}", manifest.version);
    println!("  manifest_version: {}", manifest.manifest_version);
    println!("  api_version:      {}", manifest.api_version);

    // F.B.2: contributes.detector/handler 가 opaque Value 로 전환되어 본 CLI 표시는
    // Value 의 필드를 직접 읽어 요약한다. concrete schema 검증은 manifest::load 내
    // bin glue 에서 수행하므로 여기 도달했다는 것은 schema 가 valid 함을 의미.
    let detectors = &manifest.contributes.detector;
    println!();
    println!("Detectors contributed: {}", detectors.len());
    let mut total_unsupported = 0_usize;
    for v in detectors {
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        let rules: Vec<serde_json::Value> = v
            .get("rule")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        let total = rules.len();
        let unsupported: Vec<&serde_json::Value> = rules
            .iter()
            .filter(|r| {
                let kind = r.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                !matches!(
                    kind,
                    "extension"
                        | "path_glob"
                        | "mime"
                        | "magic"
                        | "is_directory"
                        | "structure_check"
                )
            })
            .collect();
        let ok = total - unsupported.len();
        total_unsupported += unsupported.len();
        println!(
            "  - {} (rules: {} OK, {} unsupported)",
            id,
            ok,
            unsupported.len()
        );
        for rule in &unsupported {
            let kind_name = rule.get("kind").and_then(|k| k.as_str()).unwrap_or("?");
            println!(
                "      ! rule kind \"{}\" unsupported in this host version",
                kind_name
            );
        }
    }

    let handlers = &manifest.contributes.handler;
    println!();
    println!("Handlers contributed: {}", handlers.len());
    for v in handlers {
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        let detector_id = v.get("detector").and_then(|x| x.as_str()).unwrap_or("?");
        let action_summary = v
            .get("action")
            .and_then(|a| a.as_object())
            .and_then(|obj| {
                let kind = obj.get("kind").and_then(|k| k.as_str())?;
                match kind {
                    "open_surface" => Some(format!(
                        "surface \"{}\"",
                        obj.get("surface_kind")
                            .and_then(|x| x.as_str())
                            .unwrap_or("?")
                    )),
                    "ipc" => Some(format!(
                        "ipc \"{}\"",
                        obj.get("method").and_then(|x| x.as_str()).unwrap_or("?")
                    )),
                    other => Some(format!("(unknown: {other})")),
                }
            })
            .unwrap_or_else(|| "(no action)".into());
        println!(
            "  - {} → detector \"{}\" → {}",
            id, detector_id, action_summary
        );
    }

    if total_unsupported > 0 {
        println!();
        println!(
            "{} rule(s) unsupported — they will be ignored. host api_version: see Cargo.toml.",
            total_unsupported
        );
    }
    Ok(())
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
