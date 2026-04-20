use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::transport::{make_request, IpcConnection};

/// Handle the claude-hook subcommand, which maps Claude Code hook events to IPC calls.
pub fn run_claude_hook(conn: &mut IpcConnection, event: &str, surface_arg: Option<u32>) -> Result<()> {
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
            conn.send(&req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "claude-idle" }),
            );
            let result = conn.send(&req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "notification" => {
            // Claude needs input → set needs_input, then fire needs-input hook
            let req1 = make_request(
                "claude.set_needs_input",
                serde_json::json!({ "surface_id": surface_param, "needs_input": true }),
            );
            conn.send(&req1)?;

            let req2 = make_request(
                "surface.fire_hook",
                serde_json::json!({ "surface_id": surface_param, "event": "needs-input" }),
            );
            let result = conn.send(&req2)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "prompt-submit" | "session-start" | "active" => {
            // Claude became active → clear idle/needs_input
            let req = make_request(
                "claude.set_idle_state",
                serde_json::json!({ "surface_id": surface_param, "idle": false }),
            );
            let result = conn.send(&req)?;
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

const TASTY_HOOK_COMMAND: &str =
    "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook stop || true";

fn claude_settings_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(base.home_dir().join(".claude").join("settings.json"))
}

/// Install tasty Stop hook into ~/.claude/settings.json
pub fn run_claude_install() -> Result<()> {
    let path = claude_settings_path()?;

    let mut root: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content)?
    } else {
        json!({})
    };

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json root is not an object"))?
        .entry("hooks")
        .or_insert_with(|| json!({}));

    let stop_arr = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks is not an object"))?
        .entry("Stop")
        .or_insert_with(|| json!([]));

    let arr = stop_arr
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.Stop is not an array"))?;

    // Check if already installed
    let already = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("tasty claude hook stop"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if already {
        println!("Already installed");
        return Ok(());
    }

    arr.push(json!({
        "matcher": "",
        "hooks": [
            {
                "type": "command",
                "command": TASTY_HOOK_COMMAND
            }
        ]
    }));

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    println!("Installed tasty Stop hook to ~/.claude/settings.json");
    Ok(())
}

/// Uninstall tasty Stop hook from ~/.claude/settings.json
pub fn run_claude_uninstall() -> Result<()> {
    let path = claude_settings_path()?;

    if !path.exists() {
        println!("~/.claude/settings.json not found, nothing to uninstall");
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    let mut root: serde_json::Value = serde_json::from_str(&content)?;

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json root is not an object"))?;

    let Some(hooks) = root_obj.get_mut("hooks") else {
        println!("No hooks found, nothing to uninstall");
        return Ok(());
    };

    let Some(hooks_obj) = hooks.as_object_mut() else {
        println!("hooks is not an object, nothing to uninstall");
        return Ok(());
    };

    let Some(stop_val) = hooks_obj.get_mut("Stop") else {
        println!("No Stop hook found, nothing to uninstall");
        return Ok(());
    };

    let Some(arr) = stop_val.as_array_mut() else {
        println!("hooks.Stop is not an array, nothing to uninstall");
        return Ok(());
    };

    let before_len = arr.len();
    arr.retain(|entry| {
        !entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("tasty claude hook stop"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    });

    if arr.len() == before_len {
        println!("Tasty Stop hook not found, nothing to uninstall");
        return Ok(());
    }

    // Clean up empty Stop array and hooks object
    if arr.is_empty() {
        hooks_obj.remove("Stop");
    }
    if hooks_obj.is_empty() {
        root_obj.remove("hooks");
    }

    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    println!("Uninstalled tasty Stop hook from ~/.claude/settings.json");
    Ok(())
}

/// Handle the claude-wait subcommand: poll until child is idle/needs_input/exited or timeout.
pub fn run_claude_wait(conn: &mut IpcConnection, child: u32, surface_id: Option<u32>, timeout: u64) -> Result<()> {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(timeout);

    loop {
        let req = make_request(
            "claude.wait",
            serde_json::json!({ "surface_id": surface_id, "child_index": child }),
        );
        let result = conn.send(&req)?;

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
