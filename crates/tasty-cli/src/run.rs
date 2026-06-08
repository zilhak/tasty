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
    let plugins_root = tasty_host_plugin::plugin_root()?;
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
    // 루트 `--port-file` 플래그는 augmented(Cli 기반)에 그대로 포함됨. 추출해 dynamic 경로로 전달.
    let port_file = matches.get_one::<String>("port_file").cloned();
    let (top_name, _) = matches.subcommand()?;
    if !entries.iter().any(|e| e.cli.name == top_name) {
        return None;
    }
    let (request, polling, auto_wait) = match dynamic::matches_to_request(&entries, &matches) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };
    Some(match (polling, auto_wait) {
        (Some(p), _) => run_dynamic_client_polling(request, p, port_file.as_deref()),
        (None, Some(aw)) => run_dynamic_client_with_auto_wait(request, aw, port_file.as_deref()),
        (None, None) => run_dynamic_client(request, port_file.as_deref()),
    })
}

fn run_dynamic_client(
    request: tasty_ipc::protocol::JsonRpcRequest,
    port_file: Option<&str>,
) -> Result<()> {
    let port = port_file::read_port_file_from(port_file)?;
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

/// `claude spawn` / `claude tell` / `codex spawn` / `codex tell` 같이 manifest 가
/// `auto_wait` 를 선언한 명령. 1 차 IPC 응답을 line-delimited JSON 으로 출력한 뒤,
/// `--no-wait` 가 아니면 wait IPC 를 chain 호출해 terminal_states 도달까지 block —
/// wait 응답도 두 번째 JSON line 으로 출력. caller 는 마지막 line 만 파싱하면
/// wait 결과를 확보할 수 있다.
fn run_dynamic_client_with_auto_wait(
    request: tasty_ipc::protocol::JsonRpcRequest,
    aw: super::dynamic::AutoWaitPlan,
    port_file: Option<&str>,
) -> Result<()> {
    let port = port_file::read_port_file_from(port_file)?;

    // ── 1) 1 차 IPC (spawn / tell) 호출 + 응답 출력.
    let first_value = {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
            anyhow::anyhow!(
                "Could not connect to tasty instance on port {}: {}. Is tasty running?",
                port,
                e
            )
        })?;
        let mut conn = IpcConnection::new(stream)?;
        match conn.send(&request) {
            Ok(value) => value,
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
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&first_value).unwrap_or_default()
    );

    // ── 2) --no-wait 이면 여기서 종료.
    if aw.skipped {
        return Ok(());
    }

    // ── 3) wait params 구성: 응답 매핑 + 요청 매핑 + timeout.
    let mut wait_params = serde_json::Map::new();
    for (resp_key, target_key) in &aw.map_from_response {
        if let Some(v) = first_value.get(resp_key) {
            wait_params.insert(target_key.clone(), v.clone());
        }
    }
    for (req_key, target_key) in &aw.map_from_request {
        if !wait_params.contains_key(target_key)
            && let Some(v) = aw.request_params.get(req_key)
        {
            wait_params.insert(target_key.clone(), v.clone());
        }
    }
    // `--timeout` 은 원 요청 params 의 `timeout_field` 값 — wait 의 polling 이
    // 그 키로 deadline 을 읽도록 동일 키로 복사.
    let wait_timeout_key = aw
        .polling
        .timeout_field
        .clone()
        .unwrap_or_else(|| "timeout".into());
    if let Some(t) = aw.request_params.get(&aw.timeout_field) {
        wait_params.insert(wait_timeout_key, t.clone());
    }
    // matches_to_request 와 동일한 surface ↔ surface_id sync. wait IPC handler 는
    // 통상 `surface_id` 키를 기대하므로, manifest 가 응답을 `surface` 로 매핑해도
    // wait 측이 받을 수 있도록 두 키 모두 채워둔다.
    if let Some(v) = wait_params.get("surface").cloned() {
        wait_params.entry("surface_id".to_string()).or_insert(v);
    }
    if let Some(v) = wait_params.get("surface_id").cloned() {
        wait_params.entry("surface".to_string()).or_insert(v);
    }

    // ── 4) wait IPC chain. polling sense 그대로 재사용.
    let wait_req = tasty_ipc::protocol::JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: aw.method.clone(),
        params: serde_json::Value::Object(wait_params),
        id: Some(serde_json::Value::from(2)),
        session_token: request.session_token.clone(),
    };
    run_dynamic_client_polling(wait_req, aw.polling, port_file)
}

/// `tasty claude wait` 같이 manifest 가 polling 을 선언한 명령. 호스트에
/// 반복 IPC 호출 + state 확인 + terminal_states 도달 또는 timeout 까지 block.
/// timeout 도달 시 마지막 응답을 그대로 출력 (caller 가 state 보고 판단).
fn run_dynamic_client_polling(
    request: tasty_ipc::protocol::JsonRpcRequest,
    polling: tasty_plugin_manifest::PollingDecl,
    port_file: Option<&str>,
) -> Result<()> {
    use std::time::{Duration, Instant};

    let port = port_file::read_port_file_from(port_file)?;
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
        if let Some(d) = deadline
            && Instant::now() >= d
        {
            // timeout — 마지막 응답을 그대로 출력. terminal 아님을 caller 가 판단.
            if let Some(v) = last_response {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            }
            return Ok(());
        }
        std::thread::sleep(interval);
    }
}

/// Run the CLI client: connect to a running tasty instance and execute the command.
pub fn run_client(command: Commands, port_file: Option<&str>) -> Result<()> {
    // `tasty update` is standalone — no host instance required.
    if let Commands::Update(opts) = &command {
        let code = crate::commands::update::run(opts, env!("CARGO_PKG_VERSION"));
        if code == 0 {
            return Ok(());
        }
        std::process::exit(code);
    }
    // debug stream-echo uses a raw framed streaming channel, not the JSON-RPC
    // request-response path — dispatch it directly (debug builds only).
    #[cfg(debug_assertions)]
    if let Commands::Debug {
        command: super::commands::DebugCommands::StreamEcho { payload, count },
    } = &command
    {
        return crate::commands::debug::run_stream_echo(payload, *count, port_file);
    }
    // `tasty port` — read the port file and print it. Local-only (no IPC):
    // enables shell-independent remote port discovery via `ssh host tasty port`.
    if let Commands::Port = &command {
        return crate::commands::port::run_port(port_file);
    }
    // `tasty ssh-profile ...` — ssh-profiles.toml 은 client 로컬 파일이라 IPC 미경유 (단계 7).
    if let Commands::SshProfile { command } = &command {
        return crate::commands::ssh_profile::run(command);
    }
    // `--ssh` + `--force-detach` is out of scope for step 5 (remote force-detach
    // belongs to profile/management in step 7) — reject explicitly rather than
    // silently force-detaching the *local* surface.
    if let Commands::Attach {
        ssh: Some(_),
        force_detach: true,
        ..
    } = &command
    {
        anyhow::bail!(
            "--ssh 와 --force-detach 는 함께 쓸 수 없습니다 (원격 force-detach 는 미지원)."
        );
    }
    // `tasty attach <id>` (non-force) uses the raw streaming channel, not the
    // JSON-RPC path — dispatch it directly. `--force-detach` is a normal
    // request-response (attach.force_detach) so it falls through.
    // `--ssh user@host` routes through the SSH tunnel (discover port + ssh -L +
    // attach over the tunnel localport), with auto-reconnect backoff.
    if let Commands::Attach {
        surface,
        workspace,
        dump_after,
        send,
        send_to,
        raw,
        force_detach: false,
        ssh,
        profile,
        remote_tasty,
        remote_port_mode,
        no_reconnect,
        into_gui: false,
        target_port: _,
    } = &command
    {
        if surface.is_some() && workspace.is_some() {
            anyhow::bail!("surface 와 --workspace 는 함께 쓸 수 없습니다.");
        }
        if ssh.is_some() && profile.is_some() {
            anyhow::bail!("--ssh 와 --profile 는 함께 쓸 수 없습니다.");
        }
        // 단계 7 — 프로필 해석: 저장 프로필 → SshTarget(+remote_tasty/port_mode 대체).
        // None 이면 1회성 `--ssh` 경로(아래 ssh: Some(dest)).
        let profile_conn: Option<(crate::ssh::SshTarget, String, String)> = match profile {
            Some(name) => {
                let profiles = tasty_ssh_profiles::SshProfiles::load();
                let Some(p) = profiles.get(name) else {
                    anyhow::bail!(
                        "SSH 프로필 '{name}' 을 찾을 수 없습니다 (tasty ssh-profile list)."
                    );
                };
                Some((
                    crate::ssh::SshTarget::from_profile(p),
                    p.remote_tasty.clone(),
                    p.port_mode.clone(),
                ))
            }
            None => None,
        };
        // workspace 단위 attach (단계 6): 트리 N-터미널 다중화 mirror.
        if let Some(ws) = workspace {
            if *raw {
                anyhow::bail!("--raw 는 workspace attach 와 함께 쓸 수 없습니다 (다중화 스트림).");
            }
            if let Some((target, rt, pm)) = profile_conn {
                return crate::commands::attach::run_attach_workspace_ssh(
                    target,
                    &rt,
                    &pm,
                    *ws,
                    *dump_after,
                    send.as_deref(),
                    *send_to,
                    !no_reconnect,
                );
            }
            return match ssh {
                Some(dest) => crate::commands::attach::run_attach_workspace_ssh(
                    crate::ssh::SshTarget::parse(dest),
                    remote_tasty,
                    remote_port_mode,
                    *ws,
                    *dump_after,
                    send.as_deref(),
                    *send_to,
                    !no_reconnect,
                ),
                None => crate::commands::attach::run_attach_workspace(
                    *ws,
                    *dump_after,
                    send.as_deref(),
                    *send_to,
                    port_file,
                ),
            };
        }
        // surface 단위 attach (단계 4/5).
        let Some(surface) = surface else {
            anyhow::bail!("attach 대상이 필요합니다: <surface_id> 또는 --workspace <id>.");
        };
        if let Some((target, rt, pm)) = profile_conn {
            return crate::commands::attach::run_attach_ssh(
                target,
                &rt,
                &pm,
                *surface,
                *dump_after,
                send.as_deref(),
                *raw,
                !no_reconnect,
            );
        }
        return match ssh {
            Some(dest) => crate::commands::attach::run_attach_ssh(
                crate::ssh::SshTarget::parse(dest),
                remote_tasty,
                remote_port_mode,
                *surface,
                *dump_after,
                send.as_deref(),
                *raw,
                !no_reconnect,
            ),
            None => crate::commands::attach::run_attach(
                *surface,
                *dump_after,
                send.as_deref(),
                *raw,
                port_file,
            ),
        };
    }
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
            port_file,
        );
    }

    let port = port_file::read_port_file_from(port_file)?;
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
