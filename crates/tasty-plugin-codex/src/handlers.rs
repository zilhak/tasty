//! `handle_ipc_method` 내부에서 각 codex.* 메서드를 처리한다.
//!
//! 모든 호스트 호출은 `ctx.host.call(...)`을 통해 동기로 이루어진다. SDK가 worker
//! 스레드에서 dispatch하므로 main 스레드가 계속 host로부터 응답을 받을 수 있다.

use serde_json::{json, Map, Value};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::state::{ChildEntry, CodexState};

/// 응답 매핑 헬퍼: HostHandle::call 결과를 IpcMethodError로 변환.
fn host_call(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    host.call(method, params)
        .map_err(|e| IpcMethodError::new(format!("host call '{method}' failed: {e}")))
}

fn require_u32(params: &Value, key: &str) -> Result<u32, IpcMethodError> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params(&format!("missing '{key}'")))
}

fn optional_u32(params: &Value, key: &str) -> Option<u32> {
    params.get(key).and_then(|v| v.as_u64()).map(|v| v as u32)
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn resolve_parent(state: &CodexState, params: &Value) -> Result<u32, IpcMethodError> {
    if let Some(p) = optional_u32(params, "surface") {
        return Ok(p);
    }
    state.single_parent().ok_or_else(|| {
        IpcMethodError::invalid_params(
            "missing 'surface' parameter (codex has 0 or >1 parents — specify --surface)",
        )
    })
}

/// codex 명령을 PTY로 보낼 문자열을 만든다. prompt가 있으면 shell quote.
fn make_codex_command(prompt: Option<&str>) -> String {
    match prompt {
        Some(p) if !p.is_empty() => {
            let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
            format!("codex \"{escaped}\"\r")
        }
        _ => "codex\r".to_string(),
    }
}

/// shell escape: cd 등의 인자로 쓸 수 있도록 single-quote로 감싸고 내부 작은따옴표를 escape.
fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// codex 메시지를 PTY로 보낼 escape된 문자열을 만든다.
/// claude의 `handle_claude_tell`과 동일한 규칙:
/// - 줄바꿈(`\n`)은 `\` + `\r`로 변환 (codex CLI에서 newline 입력)
/// - 마지막 라인이 `\`로 끝나면 공백 추가 (`\r`이 escape되지 않도록)
/// - 끝에 `\r` 추가 = submit
fn build_tell_payload(message: &str) -> String {
    let lines: Vec<&str> = message.split('\n').collect();
    let mut pty_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        pty_text.push_str(line);
        if i < lines.len() - 1 {
            pty_text.push('\\');
            pty_text.push('\r');
        }
    }
    if pty_text.ends_with('\\') {
        pty_text.push(' ');
    }
    pty_text.push('\r');
    pty_text
}

pub fn handle_launch(
    _state: &mut CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("codex")
        .to_string();
    let directory = optional_str(&params, "directory");
    let task = optional_str(&params, "task");

    let mut ws_params = Map::new();
    ws_params.insert("name".into(), Value::String(workspace_name.clone()));
    ws_params.insert("type".into(), Value::String("terminal".into()));
    let ws_result = host_call(host, "workspace.create", Value::Object(ws_params))?;
    let workspace_id = ws_result
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "workspace.create response missing 'id': {ws_result}"
            ))
        })? as u32;
    let surface_id = ws_result
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if let Some(sid) = surface_id {
        if let Some(dir) = directory.as_deref() {
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape(&normalized);
            let cd_cmd = format!("cd {}\r", escaped);
            host_call(
                host,
                "surface.send",
                json!({"surface_id": sid, "text": cd_cmd}),
            )?;
        }
        let mut cmd = "codex".to_string();
        if let Some(t) = task.as_deref() {
            cmd.push_str(&format!(" --task {}", shell_escape(t)));
        }
        cmd.push('\r');
        host_call(
            host,
            "surface.send",
            json!({"surface_id": sid, "text": cmd}),
        )?;
    }

    Ok(json!({
        "workspace_id": workspace_id,
        "workspace_name": workspace_name,
        "surface_id": surface_id,
    }))
}

pub fn handle_parent(state: &CodexState, params: Value) -> Result<Value, IpcMethodError> {
    let child_surface = require_u32(&params, "surface")?;
    match state.parent_of_child(child_surface) {
        Some(parent_id) => Ok(json!({
            "parent_surface_id": parent_id,
            "status": "active",
        })),
        None => Ok(json!({
            "parent_surface_id": Value::Null,
            "status": "none",
        })),
    }
}

pub fn handle_tell(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let surface_id = require_u32(&params, "surface")?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'message'"))?;
    let payload = build_tell_payload(message);
    host_call(
        host,
        "surface.send",
        json!({"surface_id": surface_id, "text": payload}),
    )?;
    Ok(json!({ "sent": true, "surface_id": surface_id }))
}

pub fn handle_spawn(
    state: &mut CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface = require_u32(&params, "surface")?;
    let cwd = optional_str(&params, "cwd");
    let prompt = optional_str(&params, "prompt");
    let role = optional_str(&params, "role");
    let nickname = optional_str(&params, "nickname");

    let mut split_params = Map::new();
    split_params.insert("level".into(), Value::String("surface".into()));
    split_params.insert("target_surface".into(), Value::from(parent_surface));
    split_params.insert("direction".into(), Value::String("vertical".into()));
    split_params.insert("type".into(), Value::String("terminal".into()));
    if let Some(c) = &cwd {
        split_params.insert("cwd".into(), Value::String(c.clone()));
    }
    let split_result = host_call(host, "split", Value::Object(split_params))?;
    let new_surface_id = split_result
        .get("new_surface_id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "split response missing 'new_surface_id': {split_result}"
            ))
        })? as u32;

    let cmd = make_codex_command(prompt.as_deref());
    host_call(
        host,
        "surface.send",
        json!({"surface_id": new_surface_id, "text": cmd}),
    )?;

    let index = state.next_index_for(parent_surface);
    state.register_child(
        parent_surface,
        ChildEntry {
            child_surface_id: new_surface_id,
            index,
            cwd,
            role,
            nickname,
        },
    );
    state.save();

    Ok(json!({
        "child_surface_id": new_surface_id,
        "child_index": index,
    }))
}

pub fn handle_children(state: &CodexState, params: Value) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let children: Vec<Value> = state
        .list_children(parent)
        .iter()
        .map(|c| {
            json!({
                "index": c.index,
                "surface_id": c.child_surface_id,
                "role": c.role,
                "nickname": c.nickname,
                "state": state.state_of(c.child_surface_id),
                "cwd": c.cwd,
            })
        })
        .collect();
    Ok(json!({ "children": children }))
}

pub fn handle_wait(state: &CodexState, params: Value) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let child_index = require_u32(&params, "child")?;
    let entry = state.find_child(parent, child_index).ok_or_else(|| {
        IpcMethodError::invalid_params(&format!(
            "child {child_index} not found under surface {parent}"
        ))
    })?;
    Ok(json!({ "state": state.state_of(entry.child_surface_id) }))
}

pub fn handle_broadcast(
    state: &CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'text'"))?;
    let role_filter = params.get("role").and_then(|v| v.as_str());

    let mut sent_ids: Vec<u32> = Vec::new();
    for child in state.list_children(parent) {
        if let Some(r) = role_filter {
            if child.role.as_deref() != Some(r) {
                continue;
            }
        }
        if let Err(e) = host_call(
            host,
            "surface.send",
            json!({"surface_id": child.child_surface_id, "text": text}),
        ) {
            tracing::warn!(
                "codex broadcast surface.send (sid={}) failed: {e:?}",
                child.child_surface_id
            );
        }
        sent_ids.push(child.child_surface_id);
    }
    Ok(json!({
        "sent_count": sent_ids.len(),
        "children": sent_ids,
    }))
}

pub fn handle_kill(
    state: &mut CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let child_index = require_u32(&params, "child")?;
    let removed = state.remove_child(parent, child_index).ok_or_else(|| {
        IpcMethodError::invalid_params(&format!(
            "child {child_index} not found under surface {parent}"
        ))
    })?;
    state.save();
    host_call(
        host,
        "surface.close",
        json!({"surface_id": removed.child_surface_id}),
    )?;
    Ok(json!({
        "killed_surface_id": removed.child_surface_id,
        "child_index": removed.index,
    }))
}

pub fn handle_respawn(
    state: &mut CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let child_index = require_u32(&params, "child")?;
    let entry = state
        .find_child(parent, child_index)
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&format!(
                "child {child_index} not found under surface {parent}"
            ))
        })?
        .clone();
    let prompt = optional_str(&params, "prompt");
    let cmd = make_codex_command(prompt.as_deref());
    // Ctrl+C로 기존 프로세스 종료 후 새 codex 시작.
    host_call(
        host,
        "surface.send_combo",
        json!({"surface_id": entry.child_surface_id, "key": "c", "modifiers": ["ctrl"]}),
    )?;
    host_call(
        host,
        "surface.send",
        json!({"surface_id": entry.child_surface_id, "text": cmd}),
    )?;
    // role/nickname/cwd가 새로 들어왔으면 갱신.
    let new_role = optional_str(&params, "role");
    let new_nick = optional_str(&params, "nickname");
    let new_cwd = optional_str(&params, "cwd");
    state.update_child(parent, child_index, |e| {
        if let Some(r) = new_role {
            e.role = Some(r);
        }
        if let Some(n) = new_nick {
            e.nickname = Some(n);
        }
        if let Some(c) = new_cwd {
            e.cwd = Some(c);
        }
    });
    // idle/needs_input 초기화.
    state.set_idle(entry.child_surface_id, false);
    state.save();
    Ok(json!({
        "child_surface_id": entry.child_surface_id,
        "child_index": entry.index,
    }))
}

pub fn handle_hook(
    state: &mut CodexState,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'event'"))?;
    let surface = optional_u32(&params, "surface");
    let surface_id = surface.ok_or_else(|| {
        IpcMethodError::invalid_params("hook requires --surface to identify child")
    })?;
    match event {
        "stop" => state.set_idle(surface_id, true),
        "notification" => state.set_needs_input(surface_id, true),
        "session-end" | "subagent-stop" => state.set_idle(surface_id, true),
        "prompt-submit" | "session-start" => state.set_idle(surface_id, false),
        other => {
            return Err(IpcMethodError::invalid_params(&format!(
                "unknown hook event '{other}'"
            )));
        }
    }
    state.save();
    Ok(json!({ "ok": true, "state": state.state_of(surface_id) }))
}

pub fn handle_install(_state: &mut CodexState) -> Result<Value, IpcMethodError> {
    let path = codex_settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| IpcMethodError::new(format!("mkdir failed: {e}")))?;
    }
    let existing = read_json_or_default(&path);
    let merged = merge_install(existing);
    write_json(&path, &merged)?;
    Ok(json!({ "installed": true, "path": path.to_string_lossy() }))
}

pub fn handle_uninstall(_state: &mut CodexState) -> Result<Value, IpcMethodError> {
    let path = codex_settings_path()?;
    if !path.exists() {
        return Ok(json!({ "uninstalled": true, "path": path.to_string_lossy(), "noop": true }));
    }
    let existing = read_json_or_default(&path);
    let cleaned = remove_install(existing);
    write_json(&path, &cleaned)?;
    Ok(json!({ "uninstalled": true, "path": path.to_string_lossy() }))
}

// ───── install/uninstall helpers ─────

fn codex_settings_path() -> Result<std::path::PathBuf, IpcMethodError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| IpcMethodError::new("HOME env var not set"))?;
    Ok(std::path::PathBuf::from(home).join(".codex").join("settings.json"))
}

fn read_json_or_default(path: &std::path::Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| Value::Object(Map::new())),
        Err(_) => Value::Object(Map::new()),
    }
}

fn write_json(path: &std::path::Path, value: &Value) -> Result<(), IpcMethodError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| IpcMethodError::new(format!("encode failed: {e}")))?;
    std::fs::write(path, text).map_err(|e| IpcMethodError::new(format!("write failed: {e}")))
}

const HOOK_EVENTS: &[(&str, &str)] = &[
    ("Stop", "stop"),
    ("Notification", "notification"),
    ("SessionEnd", "session-end"),
    ("SubagentStop", "subagent-stop"),
];

fn hook_command(event_kebab: &str) -> String {
    format!(
        "tasty codex hook {event_kebab} --surface ${{TASTY_SURFACE_ID}}"
    )
}

fn merge_install(mut value: Value) -> Value {
    let obj = value.as_object_mut().cloned().unwrap_or_default();
    let mut root = Map::new();
    for (k, v) in obj.iter() {
        root.insert(k.clone(), v.clone());
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .cloned()
        .unwrap_or_default();
    let mut hooks_out = hooks;
    for (event_key, kebab) in HOOK_EVENTS {
        hooks_out.insert(
            (*event_key).to_string(),
            json!([{
                "type": "command",
                "command": hook_command(kebab),
            }]),
        );
    }
    root.insert("hooks".into(), Value::Object(hooks_out));
    Value::Object(root)
}

fn remove_install(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };
    if let Some(hooks) = obj.get_mut("hooks").and_then(|v| v.as_object_mut()) {
        for (event_key, _) in HOOK_EVENTS {
            hooks.remove(*event_key);
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_codex_command_no_prompt() {
        assert_eq!(make_codex_command(None), "codex\r");
        assert_eq!(make_codex_command(Some("")), "codex\r");
    }

    #[test]
    fn make_codex_command_with_prompt_escapes_quotes() {
        let cmd = make_codex_command(Some(r#"fix "bug" please"#));
        assert_eq!(cmd, "codex \"fix \\\"bug\\\" please\"\r");
    }

    #[test]
    fn merge_install_adds_four_events() {
        let result = merge_install(Value::Object(Map::new()));
        let hooks = result.get("hooks").unwrap().as_object().unwrap();
        for (event_key, _) in HOOK_EVENTS {
            assert!(hooks.contains_key(*event_key), "missing {event_key}");
        }
    }

    #[test]
    fn merge_install_preserves_other_keys() {
        let initial = json!({"otherKey": "value", "hooks": {"PreExisting": []}});
        let result = merge_install(initial);
        assert_eq!(result["otherKey"], "value");
        let hooks = result["hooks"].as_object().unwrap();
        assert!(hooks.contains_key("PreExisting"));
        assert!(hooks.contains_key("Stop"));
    }

    #[test]
    fn remove_install_removes_only_our_events() {
        let initial = json!({"hooks": {"Stop": [{"command": "foo"}], "Custom": [{}]}});
        let result = remove_install(initial);
        let hooks = result["hooks"].as_object().unwrap();
        assert!(!hooks.contains_key("Stop"));
        assert!(hooks.contains_key("Custom"));
    }
}
