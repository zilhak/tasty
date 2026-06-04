//! `handle_ipc_method` 내부에서 각 codex.* 메서드를 처리한다.
//!
//! 모든 호스트 호출은 `ctx.host.call(...)`을 통해 동기로 이루어진다. SDK가 worker
//! 스레드에서 dispatch하므로 main 스레드가 계속 host로부터 응답을 받을 수 있다.

use serde_json::{Map, Value, json};
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
    params.get(key).and_then(|v| v.as_str()).map(String::from)
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
///
/// `TASTY_SURFACE_ID={surface_id}` inline env prefix를 항상 박는다. 이게 없으면
/// codex 프로세스 env에 `TASTY_SURFACE_ID`가 비어, `~/.codex/settings.json`의 hook
/// 명령 (`tasty codex hook X --surface ${TASTY_SURFACE_ID}`)이 surface ID 없이
/// 실행되어 `handle_hook`이 invalid_params로 거부 → idle/needs_input 상태가 영원히
/// 갱신되지 않는다. claude plugin의 `start_claude_in_surface`와 동일한 패턴.
fn make_codex_command(surface_id: u32, prompt: Option<&str>) -> String {
    let prefix = format!("TASTY_SURFACE_ID={surface_id} ");
    match prompt {
        Some(p) if !p.is_empty() => {
            let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
            format!("{prefix}codex \"{escaped}\"\r")
        }
        _ => format!("{prefix}codex\r"),
    }
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

    // cwd 는 CLI 가 absolute path 로 정규화 + 검증해 전달 (path_kind hint).
    // 호스트 workspace.create 가 PTY working_dir 로 직접 사용 → `cd` echo 불필요.
    let mut ws_params = Map::new();
    ws_params.insert("name".into(), Value::String(workspace_name.clone()));
    ws_params.insert("type".into(), Value::String("terminal".into()));
    if let Some(dir) = directory.as_deref() {
        ws_params.insert("cwd".into(), Value::String(dir.to_string()));
    }
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
        let cmd = make_codex_command(sid, task.as_deref());
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

    let cmd = make_codex_command(new_surface_id, prompt.as_deref());
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

/// CLI 측 polling(`run_dynamic_client_polling`)이 idle/needs_input/exited 도달까지
/// 반복 호출한다. 본 함수는 그 polling tick 1개를 처리한다 — manifest의 `polling`
/// 선언이 CLI의 blocking loop를 활성화하므로 핸들러 자체는 1회 snapshot만 반환.
///
/// state가 `active` 일 때 surface 자체가 죽었는지 host에 확인한다 (claude plugin과
/// 동일한 패턴). codex가 `Stop` hook을 못 쏘고 죽거나 (e.g. SIGKILL) 사용자가 탭을
/// 닫고 plugin이 surface.closed 이벤트를 놓친 케이스에 polling이 무한 루프 빠지지
/// 않도록 방어한다.
pub fn handle_wait(
    state: &CodexState,
    host: &HostHandle,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent = resolve_parent(state, &params)?;
    let child_index = require_u32(&params, "child")?;
    let entry = state.find_child(parent, child_index).ok_or_else(|| {
        IpcMethodError::invalid_params(&format!(
            "child {child_index} not found under surface {parent}"
        ))
    })?;
    let sid = entry.child_surface_id;
    let response_state = match state.state_of(sid) {
        "active" => {
            let exists = host
                .call("surface.locate", json!({ "surface_id": sid }))
                .ok()
                .and_then(|v| v.get("exists").and_then(|e| e.as_bool()))
                .unwrap_or(true);
            if exists { "active" } else { "exited" }
        }
        other => other,
    };
    Ok(json!({ "state": response_state }))
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
        if let Some(r) = role_filter
            && child.role.as_deref() != Some(r)
        {
            continue;
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
    let new_cwd = optional_str(&params, "cwd");
    let cmd = make_codex_command(entry.child_surface_id, prompt.as_deref());

    // cwd 변경이 들어왔을 때는 PTY 자체를 새 working_dir 로 갈아끼운다.
    // Ctrl-C + 재실행만으로는 작업 디렉토리가 바뀌지 않는 문제(보고서 결함 3)
    // 를 차단. cwd 가 없으면 기존 cwd 유지 — Ctrl-C + codex 재실행만 수행.
    let effective_cwd = new_cwd.clone().or_else(|| entry.cwd.clone());
    if new_cwd.is_some() {
        let mut respawn_params = json!({ "surface_id": entry.child_surface_id });
        if let Some(c) = effective_cwd.as_deref() {
            respawn_params["cwd"] = Value::String(c.to_string());
        }
        host_call(host, "surface.respawn_terminal", respawn_params)?;
    } else {
        // 기존 동작 유지: Ctrl+C 로 기존 프로세스 종료.
        host_call(
            host,
            "surface.send_combo",
            json!({"surface_id": entry.child_surface_id, "key": "c", "modifiers": ["ctrl"]}),
        )?;
    }
    host_call(
        host,
        "surface.send",
        json!({"surface_id": entry.child_surface_id, "text": cmd}),
    )?;
    // role/nickname/cwd가 새로 들어왔으면 갱신.
    let new_role = optional_str(&params, "role");
    let new_nick = optional_str(&params, "nickname");
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

/// Codex CLI hook event 가 fire 됐을 때 호출. install 이 박은 `Stop` /
/// `UserPromptSubmit` / `SessionStart` 만 정상 처리한다.
///
/// 이전엔 `notification` / `session-end` / `subagent-stop` 도 받았으나, codex CLI
/// 0.130 에 해당 hook event 가 존재하지 않아 영원히 도착하지 않는다 (Claude Code
/// 흉내내던 잔존 코드). 외부에서 `tasty codex hook notification` 을 invoke 하면
/// invalid_params 로 거부한다.
///
/// **반환값**: 빈 객체 `{}`. CLI 의 stdout 으로 흘러나가 codex 가 직접 파싱하므로
/// codex 의 wire schema (StopCommandOutputWire / SessionStartCommandOutputWire /
/// UserPromptSubmitCommandOutputWire) 와 호환되어야 한다. 모든 필드가 optional
/// 이므로 empty object 는 "no decision, continue normally" 의미. `{"ok":true,...}`
/// 같은 자체 schema 를 반환하면 codex 가 "hook returned invalid JSON output" 으로
/// 거부한다 (side effect 는 이미 발생했지만 codex TUI 에 에러 메시지 노출).
pub fn handle_hook(state: &mut CodexState, params: Value) -> Result<Value, IpcMethodError> {
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
        "prompt-submit" | "session-start" => state.set_idle(surface_id, false),
        other => {
            return Err(IpcMethodError::invalid_params(&format!(
                "unknown hook event '{other}' (supported: stop, prompt-submit, session-start)"
            )));
        }
    }
    state.save();
    Ok(json!({}))
}

pub fn handle_install(_state: &mut CodexState) -> Result<Value, IpcMethodError> {
    let path = codex_config_toml_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| IpcMethodError::new(format!("mkdir failed: {e}")))?;
    }
    let existing = read_toml_or_default(&path);
    let merged = merge_install(existing);
    write_toml(&path, &merged)?;
    Ok(json!({
        "installed": true,
        "path": path.to_string_lossy(),
        "trust_required": true,
        "note": "Codex blocks newly-added hooks until trusted. Run `codex`, type /hooks, and approve the tasty entries.",
    }))
}

pub fn handle_uninstall(_state: &mut CodexState) -> Result<Value, IpcMethodError> {
    let path = codex_config_toml_path()?;
    if !path.exists() {
        return Ok(json!({ "uninstalled": true, "path": path.to_string_lossy(), "noop": true }));
    }
    let existing = read_toml_or_default(&path);
    let cleaned = remove_install(existing);
    write_toml(&path, &cleaned)?;
    Ok(json!({ "uninstalled": true, "path": path.to_string_lossy() }))
}

// ───── install/uninstall helpers ─────
//
// Codex CLI 0.130 의 hook 설정은 `~/.codex/config.toml` 의 `[hooks]` 섹션에 박는다.
// 이전 구현은 `~/.codex/settings.json` 에 썼으나 codex 가 그 파일은 *external agent
// config migration* (Claude Code 호환용) 경로에서만 읽고 hook dispatch 에는 쓰지
// 않는다. 그래서 install 했어도 hook 이 한 번도 fire 되지 않았다.
//
// TOML 스키마 (binary strings + 실 동작 검증):
//
// ```toml
// [[hooks.Stop]]                   # MatcherGroup 배열 entry
// # matcher = "..."                # PreToolUse 등에서 tool name regex. Stop 은 omit.
//
// [[hooks.Stop.hooks]]             # HookHandlerConfig 배열
// type = "command"                 # internally tagged enum 의 discriminator
// command = "..."
// # timeout = 5                    # optional, 초 단위
// # async = false                  # optional
// ```
//
// Codex 가 지원하는 event: Stop, PreToolUse, PostToolUse, PermissionRequest,
// PreCompact, PostCompact, SessionStart, UserPromptSubmit. tasty 는 idle/active
// 트래킹에 필요한 3 개만 박는다 (Stop, UserPromptSubmit, SessionStart).
//
// Trust gate: codex 는 새 hook entry 를 *trust* 하기 전엔 fire 하지 않고 TUI 에
// "1 hook needs review" 표시 후 `/hooks` 명령 승인을 요구한다 (`HookStateToml`
// 의 `trusted_hash` 메커니즘). install 자체는 멱등하게 entry 를 박지만, 첫
// 사용자가 `/hooks` 로 한 번 승인해야 한다 — 이는 codex CLI 의 보안 정책이므로
// 플러그인 측에서 우회 불가.

use std::path::{Path, PathBuf};

const HOOK_MARKER: &str = "tasty codex hook";

const HOOK_EVENTS: &[(&str, &str)] = &[
    ("Stop", "stop"),
    ("UserPromptSubmit", "prompt-submit"),
    ("SessionStart", "session-start"),
];

fn codex_config_toml_path() -> Result<PathBuf, IpcMethodError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| IpcMethodError::new("HOME env var not set"))?;
    Ok(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn read_toml_or_default(path: &Path) -> toml::Value {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<toml::Value>(&text)
            .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new())),
        Err(_) => toml::Value::Table(toml::map::Map::new()),
    }
}

fn write_toml(path: &Path, value: &toml::Value) -> Result<(), IpcMethodError> {
    let text = toml::to_string_pretty(value)
        .map_err(|e| IpcMethodError::new(format!("encode failed: {e}")))?;
    std::fs::write(path, text).map_err(|e| IpcMethodError::new(format!("write failed: {e}")))
}

fn hook_command(event_kebab: &str) -> String {
    // `[ -n "$VAR" ]` guard로 TASTY_SURFACE_ID 가 비어있을 때 skip. claude plugin과
    // 동일한 패턴. 가드 없으면 codex 가 `${TASTY_SURFACE_ID}` 를 빈 문자열로 치환해
    // `tasty codex hook X --surface ` 가 실행되어 invalid_params 노이즈 발생.
    format!(
        "[ -n \"$TASTY_SURFACE_ID\" ] && tasty codex hook {event_kebab} --surface $TASTY_SURFACE_ID || true"
    )
}

fn new_matcher_group(event_kebab: &str) -> toml::Value {
    let mut handler = toml::map::Map::new();
    handler.insert("type".into(), toml::Value::String("command".into()));
    handler.insert(
        "command".into(),
        toml::Value::String(hook_command(event_kebab)),
    );
    let mut group = toml::map::Map::new();
    group.insert(
        "hooks".into(),
        toml::Value::Array(vec![toml::Value::Table(handler)]),
    );
    toml::Value::Table(group)
}

fn matcher_group_has_marker(item: &toml::Value, marker: &str) -> bool {
    let Some(group) = item.as_table() else {
        return false;
    };
    let Some(hooks) = group.get("hooks").and_then(|v| v.as_array()) else {
        return false;
    };
    hooks.iter().any(|h| {
        h.as_table()
            .and_then(|t| t.get("command"))
            .and_then(|c| c.as_str())
            .map(|s| s.contains(marker))
            .unwrap_or(false)
    })
}

/// `[hooks]` 의 각 event 배열에 tasty MatcherGroup 을 멱등하게 박는다. 기존
/// non-tasty entry, 다른 키 (다른 hook event, [hooks] 외 섹션) 는 모두 보존.
fn merge_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let hooks_table = table
        .entry("hooks".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(hooks) = hooks_table.as_table_mut() else {
        return value;
    };
    for (event_key, kebab) in HOOK_EVENTS {
        let event_array = hooks
            .entry((*event_key).to_string())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        let Some(arr) = event_array.as_array_mut() else {
            continue;
        };
        // 기존 tasty marker entry 제거 후 새 entry push — 멱등.
        arr.retain(|item| !matcher_group_has_marker(item, HOOK_MARKER));
        arr.push(new_matcher_group(kebab));
    }
    value
}

fn remove_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let Some(hooks_table) = table.get_mut("hooks").and_then(|v| v.as_table_mut()) else {
        return value;
    };
    // 각 event 의 array 에서 tasty marker 가진 MatcherGroup 만 제거. `toml::map::Map`
    // 는 values_mut 가 없어 (&Map iter 만 지원) 키 목록을 떠서 우회.
    let event_keys: Vec<String> = hooks_table.keys().cloned().collect();
    for key in event_keys {
        if let Some(arr) = hooks_table.get_mut(&key).and_then(|v| v.as_array_mut()) {
            arr.retain(|item| !matcher_group_has_marker(item, HOOK_MARKER));
        }
    }
    // 빈 array 가 된 event 키 정리.
    hooks_table.retain(|_, v| !v.as_array().map(|a| a.is_empty()).unwrap_or(false));
    // [hooks] 가 텅 비면 제거.
    if hooks_table.is_empty() {
        table.remove("hooks");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_codex_command_no_prompt() {
        assert_eq!(make_codex_command(42, None), "TASTY_SURFACE_ID=42 codex\r");
        assert_eq!(
            make_codex_command(42, Some("")),
            "TASTY_SURFACE_ID=42 codex\r"
        );
    }

    #[test]
    fn make_codex_command_with_plain_prompt() {
        assert_eq!(
            make_codex_command(42, Some("hello")),
            "TASTY_SURFACE_ID=42 codex \"hello\"\r"
        );
    }

    #[test]
    fn make_codex_command_with_prompt_escapes_quotes() {
        let cmd = make_codex_command(7, Some(r#"fix "bug" please"#));
        assert_eq!(cmd, "TASTY_SURFACE_ID=7 codex \"fix \\\"bug\\\" please\"\r");
    }

    #[test]
    fn make_codex_command_with_prompt_escapes_backslash() {
        let cmd = make_codex_command(7, Some(r"path\to\file"));
        assert_eq!(cmd, "TASTY_SURFACE_ID=7 codex \"path\\\\to\\\\file\"\r");
    }

    fn parse_toml(text: &str) -> toml::Value {
        toml::from_str(text).expect("valid toml")
    }

    #[test]
    fn merge_install_adds_three_events() {
        let result = merge_install(toml::Value::Table(toml::map::Map::new()));
        let hooks = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .unwrap();
        for (event_key, _) in HOOK_EVENTS {
            assert!(hooks.contains_key(*event_key), "missing {event_key}");
            // 각 event 는 marker 가진 MatcherGroup 한 개.
            let arr = hooks.get(*event_key).unwrap().as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert!(matcher_group_has_marker(&arr[0], HOOK_MARKER));
        }
    }

    #[test]
    fn merge_install_preserves_other_keys_and_other_hook_events() {
        let initial = parse_toml(
            r#"
model = "gpt-5.5"

[projects."/path"]
trust_level = "trusted"

[[hooks.PreToolUse]]
[[hooks.PreToolUse.hooks]]
type = "command"
command = "user's own hook"
"#,
        );
        let result = merge_install(initial);
        let table = result.as_table().unwrap();
        assert_eq!(table.get("model").and_then(|v| v.as_str()), Some("gpt-5.5"));
        assert!(table.get("projects").is_some());
        let hooks = table.get("hooks").and_then(|v| v.as_table()).unwrap();
        // 사용자의 PreToolUse 는 그대로.
        let pre = hooks.get("PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert!(!matcher_group_has_marker(&pre[0], HOOK_MARKER));
        // tasty 의 Stop / UserPromptSubmit / SessionStart 가 추가됨.
        for (key, _) in HOOK_EVENTS {
            let arr = hooks.get(*key).unwrap().as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert!(matcher_group_has_marker(&arr[0], HOOK_MARKER));
        }
    }

    #[test]
    fn merge_install_is_idempotent() {
        let empty = toml::Value::Table(toml::map::Map::new());
        let once = merge_install(empty);
        let twice = merge_install(once.clone());
        assert_eq!(
            toml::to_string(&once).unwrap(),
            toml::to_string(&twice).unwrap()
        );
    }

    #[test]
    fn merge_install_keeps_coexisting_non_tasty_stop_hook() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "user wrote this Stop hook themselves"
"#,
        );
        let result = merge_install(initial);
        let stop = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        // 사용자 hook + tasty hook = 2 entries.
        assert_eq!(stop.len(), 2);
        assert_eq!(
            stop.iter()
                .filter(|i| matcher_group_has_marker(i, HOOK_MARKER))
                .count(),
            1
        );
    }

    #[test]
    fn remove_install_removes_only_tasty_marker_entries() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "keep me — not tasty"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "tasty codex hook stop --surface $TASTY_SURFACE_ID"
"#,
        );
        let result = remove_install(initial);
        let stop = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(stop.len(), 1);
        assert!(!matcher_group_has_marker(&stop[0], HOOK_MARKER));
    }

    #[test]
    fn remove_install_drops_empty_hooks_block() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "tasty codex hook stop"
"#,
        );
        let result = remove_install(initial);
        // [hooks] 가 통째로 사라져야 함.
        assert!(result.as_table().unwrap().get("hooks").is_none());
    }

    #[test]
    fn handle_hook_rejects_unsupported_events() {
        // notification / session-end / subagent-stop 은 codex 가 fire 하지 않으므로
        // handle_hook 도 거부 (silent no-op 대신 invalid_params).
        let mut s = CodexState::default();
        let err = handle_hook(&mut s, json!({"event": "notification", "surface": 1})).unwrap_err();
        assert!(format!("{err:?}").contains("unknown hook event"));
    }
}
