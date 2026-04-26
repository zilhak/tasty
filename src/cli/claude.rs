use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};

use super::transport::{IpcConnection, make_request};

/// Handle the claude-hook subcommand, which maps Claude Code hook events to IPC calls.
pub fn run_claude_hook(
    conn: &mut IpcConnection,
    event: &str,
    surface_arg: Option<u32>,
) -> Result<()> {
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
        "stop" | "session-end" | "subagent-stop" => {
            // Claude finished (main agent stopped, session ended, or sub-agent stopped)
            // → set idle and fire claude-idle hook.
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
                "Unknown claude-hook event: '{}'. Use: stop, notification, session-end, subagent-stop, prompt-submit, session-start",
                event
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// install / uninstall helpers
//
// 함수 단위로 깔끔하게 분리해 두어, 동일한 파일을 손대는 다른 작업과의
// 머지 충돌을 줄인다. `run_claude_install` / `run_claude_uninstall`은 이 헬퍼들의
// 얇은 래퍼다.
// ─────────────────────────────────────────────────────────────────────────────

/// tasty가 자동으로 등록하는 Claude Code hook 이벤트 목록.
/// `(claude_event_name, tasty_hook_token)` 형태.
///
/// - `Stop`: 메인 에이전트 응답 종료 (idle 신호의 핵심)
/// - `Notification`: 권한 요청 / idle 알림 (needs-input 신호의 핵심)
/// - `SessionEnd`: 세션 종료 (exited 정확도 향상)
/// - `SubagentStop`: subagent(Task tool) 종료 (idle 정확도 향상)
const MANAGED_HOOKS: &[(&str, &str)] = &[
    ("Stop", "stop"),
    ("Notification", "notification"),
    ("SessionEnd", "session-end"),
    ("SubagentStop", "subagent-stop"),
];

/// `entry_matches_marker`가 식별자로 사용하는 substring.
/// 예: "tasty claude hook stop" → settings.json 안의 command 필드에 이 substring이
/// 포함되면 tasty가 등록한 hook entry로 간주한다.
fn tasty_hook_marker(event_token: &str) -> String {
    format!("tasty claude hook {}", event_token)
}

/// settings.json에 실제로 기록되는 명령 문자열.
/// `TASTY_SURFACE_ID`가 없는 환경(claude를 tasty 외부에서 실행 중인 경우)에서는
/// 무조건 성공 종료(`true`)하도록 가드를 걸어 Claude 측에 영향을 주지 않는다.
fn tasty_hook_command(event_token: &str) -> String {
    format!(
        "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook {} || true",
        event_token
    )
}

fn claude_settings_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(base.home_dir().join(".claude").join("settings.json"))
}

/// hooks.<Event> 배열의 한 entry가 주어진 marker를 가진 tasty entry인지 판단.
fn entry_matches_marker(entry: &Value, marker: &str) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.contains(marker))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// settings.json 루트 값에서 특정 hook 이벤트 + marker가 이미 설치돼 있는지 검사.
fn is_marker_installed_in_value(root: &Value, event_name: &str, marker: &str) -> bool {
    let Some(hooks) = root.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    let Some(arr) = hooks.get(event_name).and_then(|v| v.as_array()) else {
        return false;
    };
    arr.iter().any(|entry| entry_matches_marker(entry, marker))
}

/// Read `~/.claude/settings.json` and return whether the tasty Stop hook is installed.
///
/// Returns `Ok(false)` if the file does not exist. Propagates I/O and JSON parse errors so the
/// caller can surface them in a guidance message. Stop hook은 4종 중 idle 신호의 핵심이므로
/// `tasty claude wait`의 사전 점검 기준으로 사용한다.
pub fn is_tasty_stop_hook_installed() -> Result<bool> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&path)?;
    let root: Value = serde_json::from_str(&content)?;
    let marker = tasty_hook_marker("stop");
    Ok(is_marker_installed_in_value(&root, "Stop", &marker))
}

/// settings.json 루트 값에 4종 hook을 idempotent하게 추가.
/// 이미 있으면 건너뛰고, 추가된 이벤트 이름 목록을 반환한다.
fn install_hooks_in_value(root: &mut Value) -> Result<Vec<&'static str>> {
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json root is not an object"))?;

    let hooks_obj = root_obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks is not an object"))?;

    let mut added: Vec<&'static str> = Vec::new();

    for (event_name, event_token) in MANAGED_HOOKS {
        let marker = tasty_hook_marker(event_token);
        let command = tasty_hook_command(event_token);

        let arr = hooks_obj
            .entry((*event_name).to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("hooks.{} is not an array", event_name))?;

        let already = arr.iter().any(|entry| entry_matches_marker(entry, &marker));
        if already {
            continue;
        }

        arr.push(json!({
            "matcher": "",
            "hooks": [
                {
                    "type": "command",
                    "command": command
                }
            ]
        }));
        added.push(*event_name);
    }

    Ok(added)
}

/// settings.json 루트 값에서 4종 hook의 tasty entry를 제거.
/// 빈 이벤트 배열과 빈 hooks 객체도 정리한다. 제거된 이벤트 이름 목록을 반환.
fn uninstall_hooks_from_value(root: &mut Value) -> Vec<&'static str> {
    let Some(root_obj) = root.as_object_mut() else {
        return Vec::new();
    };
    let Some(hooks) = root_obj.get_mut("hooks") else {
        return Vec::new();
    };
    let Some(hooks_obj) = hooks.as_object_mut() else {
        return Vec::new();
    };

    let mut removed: Vec<&'static str> = Vec::new();

    for (event_name, event_token) in MANAGED_HOOKS {
        let marker = tasty_hook_marker(event_token);

        let Some(arr) = hooks_obj.get_mut(*event_name).and_then(|v| v.as_array_mut()) else {
            continue;
        };

        let before_len = arr.len();
        arr.retain(|entry| !entry_matches_marker(entry, &marker));
        let changed = arr.len() != before_len;

        if changed {
            removed.push(*event_name);
        }

        if arr.is_empty() {
            hooks_obj.remove(*event_name);
        }
    }

    if hooks_obj.is_empty() {
        root_obj.remove("hooks");
    }

    removed
}

/// Install tasty Claude Code hooks (Stop/Notification/SessionEnd/SubagentStop)
/// into ~/.claude/settings.json. Idempotent.
pub fn run_claude_install() -> Result<()> {
    let path = claude_settings_path()?;

    let mut root: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content)?
    } else {
        json!({})
    };

    let added = install_hooks_in_value(&mut root)?;

    if added.is_empty() {
        println!("Already installed");
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    println!(
        "Installed tasty Claude hooks to ~/.claude/settings.json: {}",
        added.join(", ")
    );
    Ok(())
}

/// Uninstall tasty Claude Code hooks from ~/.claude/settings.json.
pub fn run_claude_uninstall() -> Result<()> {
    let path = claude_settings_path()?;

    if !path.exists() {
        println!("~/.claude/settings.json not found, nothing to uninstall");
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    let mut root: Value = serde_json::from_str(&content)?;

    let removed = uninstall_hooks_from_value(&mut root);

    if removed.is_empty() {
        println!("Tasty hooks not found, nothing to uninstall");
        return Ok(());
    }

    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    println!(
        "Uninstalled tasty Claude hooks from ~/.claude/settings.json: {}",
        removed.join(", ")
    );
    Ok(())
}

/// Handle the claude-wait subcommand: poll until child is idle/needs_input/exited or timeout.
pub fn run_claude_wait(
    conn: &mut IpcConnection,
    child: u32,
    surface_id: Option<u32>,
    timeout: u64,
) -> Result<()> {
    use std::time::{Duration, Instant};

    match is_tasty_stop_hook_installed() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!(
                "error: tasty Stop hook is not installed in ~/.claude/settings.json.\n       \
                 Without it, Claude Code idle/needs-input events are not delivered to tasty\n       \
                 and `tasty claude wait` cannot complete.\n\n       \
                 Run:\n           tasty claude install\n\n       \
                 Then retry this command."
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!(
                "error: failed to read ~/.claude/settings.json while checking tasty Stop hook: {e}\n       \
                 Fix the settings file (or run `tasty claude install`) and retry."
            );
            std::process::exit(1);
        }
    }

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
                    eprintln!(
                        "Timeout: child {} did not reach a terminal state within {}s",
                        child, timeout
                    );
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_managed_entries(root: &Value, event_name: &str, marker: &str) -> usize {
        root.get("hooks")
            .and_then(|h| h.as_object())
            .and_then(|h| h.get(event_name))
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|e| entry_matches_marker(e, marker))
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn install_in_empty_value_adds_all_four_events() {
        let mut root = json!({});
        let added = install_hooks_in_value(&mut root).expect("install");
        assert_eq!(added.len(), 4);
        for (event_name, token) in MANAGED_HOOKS {
            let marker = tasty_hook_marker(token);
            assert_eq!(
                count_managed_entries(&root, event_name, &marker),
                1,
                "missing event {} after install",
                event_name
            );
        }
    }

    #[test]
    fn install_is_idempotent() {
        let mut root = json!({});
        let _ = install_hooks_in_value(&mut root).expect("install 1");
        let added2 = install_hooks_in_value(&mut root).expect("install 2");
        assert!(added2.is_empty(), "second install should add nothing");
        for (event_name, token) in MANAGED_HOOKS {
            let marker = tasty_hook_marker(token);
            assert_eq!(count_managed_entries(&root, event_name, &marker), 1);
        }
    }

    #[test]
    fn install_preserves_other_hooks() {
        let mut root = json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo user" }] }
                ],
                "Stop": [
                    { "matcher": "", "hooks": [{ "type": "command", "command": "echo user-stop" }] }
                ]
            }
        });
        let _ = install_hooks_in_value(&mut root).expect("install");

        // user PreToolUse 보존
        assert_eq!(
            root["hooks"]["PreToolUse"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            1
        );
        // user Stop entry + tasty Stop entry 둘 다 존재
        let stop_arr = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_arr.len(), 2);
    }

    #[test]
    fn uninstall_removes_all_four() {
        let mut root = json!({});
        let _ = install_hooks_in_value(&mut root).expect("install");
        let removed = uninstall_hooks_from_value(&mut root);
        assert_eq!(removed.len(), 4);
        // hooks 객체 자체가 사라져야 함
        assert!(root.get("hooks").is_none(), "empty hooks should be removed");
    }

    #[test]
    fn uninstall_preserves_user_entries() {
        let mut root = json!({
            "hooks": {
                "Stop": [
                    { "matcher": "", "hooks": [{ "type": "command", "command": "echo user-stop" }] }
                ]
            }
        });
        let _ = install_hooks_in_value(&mut root).expect("install");
        let _ = uninstall_hooks_from_value(&mut root);
        // 사용자 entry는 그대로 남아있어야 한다
        let stop_arr = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_arr.len(), 1);
        let cmd = stop_arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("user-stop"));
    }

    #[test]
    fn uninstall_on_empty_settings_returns_empty() {
        let mut root = json!({});
        let removed = uninstall_hooks_from_value(&mut root);
        assert!(removed.is_empty());
    }

    #[test]
    fn is_marker_installed_in_value_works() {
        let mut root = json!({});
        let marker = tasty_hook_marker("stop");
        assert!(!is_marker_installed_in_value(&root, "Stop", &marker));
        let _ = install_hooks_in_value(&mut root).expect("install");
        assert!(is_marker_installed_in_value(&root, "Stop", &marker));
    }
}
