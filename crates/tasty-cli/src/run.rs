//! Plugin CLI fallback + client mode runner.

use std::net::TcpStream;

use anyhow::Result;

use tasty_ipc::port_file;

use super::Commands;
use super::commands::PluginCommands;
use super::dynamic;
use super::format::format_output;
use super::plugin::run_plugin_logs;
use super::request::command_to_request;
use super::transport::IpcConnection;

pub fn try_run_plugin_cli() -> Option<Result<()>> {
    let plugins_root = match tasty_host_plugin::plugin_root() {
        Some(p) => p,
        None => return None,
    };
    let entries = dynamic::discover_plugin_clis(&plugins_root);
    if entries.is_empty() {
        return None;
    }
    // 사용자가 입력한 첫 인자가 plugin command 이름인지 확인. plugin 명령이 맞다면
    // clap 에러도 자체 출력으로 처리한다 (정적 CLI의 "unrecognized subcommand"가
    // 대신 뜨면 안 됨).
    let first_arg = std::env::args().nth(1);
    let is_plugin_cmd = first_arg
        .as_deref()
        .map(|name| entries.iter().any(|e| e.cli.name == name))
        .unwrap_or(false);
    let augmented = dynamic::build_augmented_cli(&entries);
    let matches = match augmented.try_get_matches() {
        Ok(m) => m,
        Err(err) => {
            if is_plugin_cmd {
                err.exit();
            }
            return None;
        }
    };
    let (top_name, _) = matches.subcommand()?;
    if !entries.iter().any(|e| e.cli.name == top_name) {
        return None;
    }
    let (request, polling) = match dynamic::matches_to_request(&entries, &matches) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    Some(match polling {
        Some(p) => run_dynamic_client_polling(request, p),
        None => run_dynamic_client(request),
    })
}

fn run_dynamic_client(request: tasty_ipc::protocol::JsonRpcRequest) -> Result<()> {
    let port = port_file::read_port_file()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;
    let mut conn = IpcConnection::new(stream)?;
    match conn.send(&request) {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            if let Some(rest) = msg.strip_prefix("Error (") {
                eprintln!("Error ({}", rest);
            } else {
                eprintln!("{}", msg);
            }
            std::process::exit(1);
        }
    }
}

/// `tasty claude wait` 같이 manifest 가 polling 을 선언한 명령. 호스트에
/// 반복 IPC 호출 + state 확인 + terminal_states 도달 또는 timeout 까지 block.
/// timeout 도달 시 마지막 응답을 그대로 출력 (caller 가 state 보고 판단).
fn run_dynamic_client_polling(
    request: tasty_ipc::protocol::JsonRpcRequest,
    polling: tasty_plugin_manifest::PollingDecl,
) -> Result<()> {
    use std::time::{Duration, Instant};

    let port = port_file::read_port_file()?;
    let interval = Duration::from_millis(polling.interval_ms);
    // timeout_field 가 manifest 에 선언되어 있으면 request.params 에서 그 값 (초)
    // 을 deadline 으로 사용. 없으면 무한 대기.
    let deadline = polling.timeout_field.as_ref().and_then(|field| {
        request
            .params
            .get(field)
            .and_then(|v| v.as_u64())
            .map(|secs| Instant::now() + Duration::from_secs(secs))
    });

    // 첫 None 할당은 loop body 의 `last_response = Some(value);` 가 항상 덮어쓰므로
    // dead store 이지만, deadline 분기서 읽으려면 mutable 변수 선언이 필요. suppress.
    #[allow(unused_assignments)]
    let mut last_response: Option<serde_json::Value> = None;
    loop {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
            anyhow::anyhow!(
                "Could not connect to tasty instance on port {}: {}. Is tasty running?",
                port,
                e
            )
        })?;
        let mut conn = IpcConnection::new(stream)?;
        match conn.send(&request) {
            Ok(value) => {
                let reached = value
                    .get(&polling.state_field)
                    .and_then(|v| v.as_str())
                    .map(|s| polling.terminal_states.iter().any(|t| t == s))
                    .unwrap_or(false);
                if reached {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).unwrap_or_default()
                    );
                    return Ok(());
                }
                last_response = Some(value);
            }
            Err(e) => {
                // IPC 자체 에러는 polling 의미 없음 — 그대로 종료.
                let msg = e.to_string();
                if let Some(rest) = msg.strip_prefix("Error (") {
                    eprintln!("Error ({}", rest);
                } else {
                    eprintln!("{}", msg);
                }
                std::process::exit(1);
            }
        }
        if let Some(d) = deadline {
            if Instant::now() >= d {
                // timeout — 마지막 응답을 그대로 출력. terminal 아님을 caller 가 판단.
                if let Some(v) = last_response {
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                }
                return Ok(());
            }
        }
        std::thread::sleep(interval);
    }
}

/// Run the CLI client: connect to a running tasty instance and execute the command.
pub fn run_client(command: Commands) -> Result<()> {
    // plugin logs is local-only — read the log file directly.
    if let Commands::Plugin {
        command: PluginCommands::Logs { id, follow },
    } = &command
    {
        return run_plugin_logs(id, *follow);
    }
    // plugin doctor is local-only — read manifest from disk, no IPC needed.
    if let Commands::Plugin {
        command: PluginCommands::Doctor { id },
    } = &command
    {
        return crate::plugin::run_plugin_doctor(id);
    }
    // plugin audit-follow is a polling loop over plugin.audit_follow IPC.
    if let Commands::Plugin {
        command:
            PluginCommands::AuditFollow {
                caller_kind,
                caller_id,
                method_prefix,
                decision,
                batch,
                interval_ms,
            },
    } = &command
    {
        return crate::plugin::run_audit_follow(
            caller_kind.as_deref(),
            caller_id.as_deref(),
            method_prefix.as_deref(),
            decision.as_deref(),
            *batch,
            *interval_ms,
        );
    }

    let port = port_file::read_port_file()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;

    let mut conn = IpcConnection::new(stream)?;

    let request = command_to_request(&command);
    let result = conn.send(&request);

    match result {
        Ok(value) => format_output(&command, &value),
        Err(e) => {
            let msg = e.to_string();
            if let Some(rest) = msg.strip_prefix("Error (") {
                eprintln!("Error ({}", rest);
            } else {
                eprintln!("{}", msg);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}
