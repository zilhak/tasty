use std::net::TcpStream;

use anyhow::Result;

use super::transport::{make_request, send_request};

/// Handle the claude-hook subcommand, which maps Claude Code hook events to IPC calls.
pub fn run_claude_hook(stream: &mut TcpStream, event: &str, surface_arg: Option<u32>) -> Result<()> {
    // Resolve surface_id: --surface arg > TASTY_SURFACE_ID env var > null (server uses focused)
    let surface_id = surface_arg.or_else(|| {
        std::env::var("TASTY_SURFACE_ID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
    });

    let surface_param = match surface_id {
        Some(sid) => serde_json::json!(sid),
        None => serde_json::Value::Null,
    };

    match event {
        "stop" => {
            // Claude stopped → set idle, then fire claude-idle hook
            let req1 = make_request(
                "claude.set_idle_state",
                serde_json::json!({ "surface_id": surface_param, "idle": true }),
            );
            send_request(stream, &req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "claude-idle" }),
            );
            let result = send_request(stream, &req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "notification" => {
            // Claude needs input → set needs_input, then fire needs-input hook
            let req1 = make_request(
                "claude.set_needs_input",
                serde_json::json!({ "surface_id": surface_param, "needs_input": true }),
            );
            send_request(stream, &req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "needs-input" }),
            );
            let result = send_request(stream, &req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "prompt-submit" | "session-start" | "active" => {
            // Claude became active → clear idle/needs_input
            let req = make_request(
                "claude.set_idle_state",
                serde_json::json!({ "surface_id": surface_param, "idle": false }),
            );
            let result = send_request(stream, &req)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            eprintln!(
                "Unknown claude-hook event: '{}'. Use: stop, notification, prompt-submit, session-start",
                event
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Handle the claude-wait subcommand: poll until child is idle/needs_input/exited or timeout.
pub fn run_claude_wait(stream: &mut TcpStream, child: u32, timeout: u64) -> Result<()> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(timeout);

    loop {
        let req = make_request(
            "claude.wait",
            serde_json::json!({ "child_surface_id": child }),
        );
        let result = send_request(stream, &req)?;

        let state = result
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("active")
            .to_string();

        match state.as_str() {
            "idle" | "needs_input" | "exited" => {
                println!("{}", serde_json::to_string_pretty(&result)?);
                return Ok(());
            }
            _ => {
                if Instant::now() >= deadline {
                    eprintln!("Timeout: child {} did not reach a terminal state within {}s", child, timeout);
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}
