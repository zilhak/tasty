//! `~/.claude/settings.json` 머지 로직.
//!
//! 호스트 `src/cli/claude.rs`의 install/uninstall helper를 1:1 옮긴 것.
//! 두 곳을 동시에 유지해 cutover 직전까지 회귀를 막는다. step 04 cutover에서
//! 호스트 측은 제거되고 본 모듈이 단일 출처가 된다.
//!
//! `is_tasty_stop_hook_installed`는 tasty Stop hook 설치 여부를 점검하는
//! 별도 노출 함수다.

#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};

/// tasty가 자동으로 등록하는 Claude Code hook 이벤트 목록.
/// `(claude_event_name, tasty_hook_token)` 형태. 호스트 `MANAGED_HOOKS`와 동일.
///
/// `UserPromptSubmit` 은 child 가 *2 번째 이후 prompt* 를 받을 때 ClaudeState 의
/// idle=true (직전 Stop hook 잔재) 를 clear 하는 데 필수. 미등록 시 multi-round
/// 대화에서 `claude.wait` polling 이 *진짜 active 인 child* 를 idle 로 잘못 보고
/// transient state bug 발생 (Phase G.A 진행 중 확인).
pub const MANAGED_HOOKS: &[(&str, &str)] = &[
    ("Stop", "stop"),
    ("Notification", "notification"),
    ("SessionEnd", "session-end"),
    ("SubagentStop", "subagent-stop"),
    ("SessionStart", "session-start"),
    ("UserPromptSubmit", "prompt-submit"),
];

/// `entry_matches_marker`가 식별자로 사용하는 substring.
fn tasty_hook_marker(event_token: &str) -> String {
    format!("tasty claude hook {}", event_token)
}

/// settings.json에 실제로 기록되는 명령 문자열.
/// 호스트 코드와 byte-for-byte 동일해야 사용자가 install/uninstall을 반복해도
/// 같은 entry가 식별되어 idempotent하게 동작한다.
///
/// `session_id` 와 `message` 등 hook 별 가변 데이터는 Claude Code 가 stdin 으로
/// 흘려보내는 JSON payload 에서 `tasty claude hook` CLI 가 직접 읽어 채운다
/// (매니페스트 `stdin_json = true` + `stdin_field` 매핑). 그래서 명령 문자열은
/// 어느 event 에서도 동일한 형태로 충분하다.
fn tasty_hook_command(event_token: &str) -> String {
    format!(
        "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook {} || true",
        event_token
    )
}

pub fn claude_settings_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(base.home_dir().join(".claude").join("settings.json"))
}

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

pub fn is_marker_installed_in_value(root: &Value, event_name: &str, marker: &str) -> bool {
    let Some(hooks) = root.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    let Some(arr) = hooks.get(event_name).and_then(|v| v.as_array()) else {
        return false;
    };
    arr.iter().any(|entry| entry_matches_marker(entry, marker))
}

/// `~/.claude/settings.json`을 읽어 tasty Stop hook이 설치돼 있는지. 파일이
/// 없으면 false.
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

/// settings.json 루트 값에 hook을 idempotent하게 추가.
pub fn install_hooks_in_value(root: &mut Value) -> Result<Vec<&'static str>> {
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

        // 기존에 marker 가 일치하는 entry 가 있으면, 그 안의 hook 명령 문자열만
        // canonical 한 새 값으로 갱신한다. 옛 버전이 설치한 잘못된 명령
        // (예: `tasty claude hook session-start --session ${CLAUDE_SESSION_ID}`)
        // 이 그대로 남아 hook 이 실패하던 회귀가 다시 재현되지 않게, install 을
        // 재실행하면 자동으로 최신 형태로 upgrade 된다.
        let mut upgraded = false;
        for entry in arr.iter_mut() {
            if !entry_matches_marker(entry, &marker) {
                continue;
            }
            upgraded = true;
            if let Some(hooks) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                for h in hooks.iter_mut() {
                    let needs_update = h
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains(&marker) && c != command)
                        .unwrap_or(false);
                    if needs_update && let Some(obj) = h.as_object_mut() {
                        obj.insert("command".into(), Value::String(command.clone()));
                    }
                }
            }
        }
        if upgraded {
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

/// settings.json 루트 값에서 tasty hook entry를 제거.
pub fn uninstall_hooks_from_value(root: &mut Value) -> Vec<&'static str> {
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

        let Some(arr) = hooks_obj
            .get_mut(*event_name)
            .and_then(|v| v.as_array_mut())
        else {
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

/// `claude.install` IPC 핸들러. ~/.claude/settings.json을 idempotent하게 갱신.
pub fn run_install() -> Result<Vec<&'static str>> {
    let path = claude_settings_path()?;

    let mut root: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content)?
    } else {
        json!({})
    };

    let added = install_hooks_in_value(&mut root)?;
    if added.is_empty() {
        return Ok(added);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    Ok(added)
}

/// `claude.uninstall` IPC 핸들러. 파일이 없으면 빈 목록 반환.
pub fn run_uninstall() -> Result<Vec<&'static str>> {
    let path = claude_settings_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path)?;
    let mut root: Value = serde_json::from_str(&content)?;
    let removed = uninstall_hooks_from_value(&mut root);

    if removed.is_empty() {
        return Ok(removed);
    }
    let output = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, output)?;
    Ok(removed)
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
    fn install_in_empty_value_adds_all_events() {
        let mut root = json!({});
        let added = install_hooks_in_value(&mut root).expect("install");
        assert_eq!(added.len(), MANAGED_HOOKS.len());
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
    fn install_upgrades_stale_command() {
        // 옛 install 이 남긴 잘못된 SessionStart command 문자열이, 재 install 시
        // canonical 한 새 형태로 자동 갱신되어야 한다. 사용자가 uninstall→install
        // 수작업을 하지 않아도 복원이 정상화되도록.
        let stale_command = "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook session-start --session ${CLAUDE_SESSION_ID} || true";
        let mut root = json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{ "type": "command", "command": stale_command }]
                    }
                ]
            }
        });
        let added = install_hooks_in_value(&mut root).expect("install");
        // 신규 추가가 아니라 in-place upgrade 라서 added 에 SessionStart 는 없다.
        assert!(!added.contains(&"SessionStart"));
        let arr = root["hooks"]["SessionStart"].as_array().unwrap();
        // 중복 entry 가 추가되지 않고 한 개만 남는다.
        let tasty_count = arr
            .iter()
            .filter(|e| {
                e["hooks"][0]["command"]
                    .as_str()
                    .map(|c| c.contains("tasty claude hook session-start"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            tasty_count, 1,
            "stale entry should be upgraded, not duplicated"
        );
        let cmd = arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, tasty_hook_command("session-start"));
        assert!(!cmd.contains("${CLAUDE_SESSION_ID}"));
    }

    #[test]
    fn install_is_idempotent() {
        let mut root = json!({});
        install_hooks_in_value(&mut root).expect("install 1");
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
        install_hooks_in_value(&mut root).expect("install");

        assert_eq!(
            root["hooks"]["PreToolUse"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            1
        );
        let stop_arr = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_arr.len(), 2);
    }

    #[test]
    fn uninstall_removes_all() {
        let mut root = json!({});
        install_hooks_in_value(&mut root).expect("install");
        let removed = uninstall_hooks_from_value(&mut root);
        assert_eq!(removed.len(), MANAGED_HOOKS.len());
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
        install_hooks_in_value(&mut root).expect("install");
        // 반환값(제거된 항목 목록)은 다음 assert가 root 구조로 검증하므로 무시.
        uninstall_hooks_from_value(&mut root);
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
        install_hooks_in_value(&mut root).expect("install");
        assert!(is_marker_installed_in_value(&root, "Stop", &marker));
    }

    /// 모든 hook event 의 명령 문자열이 동일한 단순 형태인지 검증한다. session_id
    /// 등 가변 데이터는 stdin JSON 으로 흐르므로, 명령 자체는 event 토큰만 다르다.
    /// (옛 버전이 `--session ${CLAUDE_SESSION_ID}` 같은 쉘 확장에 의존하다 동작
    /// 실패했던 회귀를 막는다.)
    #[test]
    fn hook_command_matches_host_format() {
        assert_eq!(
            tasty_hook_command("stop"),
            "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook stop || true"
        );
        assert_eq!(
            tasty_hook_command("session-start"),
            "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook session-start || true"
        );
        assert_eq!(
            tasty_hook_command("notification"),
            "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude hook notification || true"
        );
    }

    /// SessionStart hook 도 다른 hook 과 동일한 단순 명령으로 설치되어야 한다.
    #[test]
    fn session_start_hook_uses_simple_command() {
        let mut root = json!({});
        install_hooks_in_value(&mut root).expect("install");
        let arr = root["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let cmd = arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            !cmd.contains("${CLAUDE_SESSION_ID}"),
            "SessionStart hook must not contain shell-expansion placeholder anymore (session_id arrives via stdin JSON), got: {}",
            cmd
        );
        assert!(
            !cmd.contains("--session"),
            "SessionStart hook must not pass --session via CLI (session_id is read from stdin), got: {}",
            cmd
        );
        assert!(cmd.contains("tasty claude hook session-start"));
    }
}
