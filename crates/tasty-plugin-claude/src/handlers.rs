//! `tasty-claude` 의 IPC handler fn 들 — 외부 plugin SDK 진입점.
//!
//! 자식 terminal 관리(spawn/tell/wait/children/parent/kill/respawn/broadcast)는
//! 호스트가 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 **위임**한다. 이
//! plugin 은 더 이상 자체 child registry 를 보유하지 않는다(호스트 registry 가 단일
//! SoT). claude **특화**만 여기 남는다:
//! - `start_claude_in_surface` / `issue_session_token` — session token + agent id +
//!   TASTY_SURFACE_ID inline env 를 박은 기동 명령 (spawn/respawn 이 소비).
//! - `build_launch_command` / `handle_launch` — 새 workspace 기동 + error_scan 등록.
//! - `handle_children` — 호스트 registry 목록에 `surface.foreground_process` 를 덧씌움.
//! - wall-time 텔레메트리 타이밍(`ClaudeState`) — hook.rs 가 소비.

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::error_scan::ErrorScanner;

pub(crate) fn require_surface_id(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'surface_id' parameter"))
}

fn require_child_index(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("child_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'child_index' parameter"))
}

/// 요청 params 에서 지정한 키들을 존재할 때만 그대로 새 Map 에 복사한다. CLI 인자를
/// 호스트 `terminal.*` 로 pass-through 하는 용도. claude CLI 는 `surface` 인자에
/// 대해 `surface`/`surface_id` 두 키를 모두 주입하므로(호스트 terminal.* 는
/// `surface` 를 읽음) `surface` 를 복사한다.
fn forward(params: &Value, keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = params.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

fn host_call(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    host.call(method, params).map_err(IpcMethodError::from)
}

/// 호스트 registry 목록(`terminal.children`)에 `surface.foreground_process` 로
/// 각 자식의 PTY 전경 프로세스를 덧씌운다. claude 특화 필드명(`child_surface_id`)을
/// 보존하기 위해 호스트 응답(`surface_id`)을 remap 한다. 응답은 bare 배열(claude
/// CLI 출력 shape).
pub(crate) fn handle_children(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let resp = host_call(
        host,
        "terminal.children",
        Value::Object(forward(params, &["surface"])),
    )?;
    let list = resp
        .get("children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut entries: Vec<Value> = list
        .iter()
        .map(|c| {
            json!({
                "child_surface_id": c.get("surface_id").cloned().unwrap_or(Value::Null),
                "index": c.get("index").cloned().unwrap_or(Value::Null),
                "cwd": c.get("cwd").cloned().unwrap_or(Value::Null),
                "role": c.get("role").cloned().unwrap_or(Value::Null),
                "nickname": c.get("nickname").cloned().unwrap_or(Value::Null),
                "state": c.get("state").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    for entry in &mut entries {
        let Some(sid) = entry
            .get("child_surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
        else {
            continue;
        };
        if let Ok(resp) = host.call("surface.foreground_process", json!({ "surface_id": sid })) {
            if let Some(name) = resp.get("name").and_then(|v| v.as_str()) {
                entry["foreground_process"] = json!(name);
            }
            if let Some(pid) = resp.get("pid").and_then(|v| v.as_u64()) {
                entry["foreground_pid"] = json!(pid);
            }
        }
    }
    Ok(json!(entries))
}

/// 한 번의 호출은 1회 상태 스냅샷이며, CLI 측 polling 이 idle/needs_input/exited
/// 도달까지 반복 호출한다. 상태 판정은 호스트 `terminal.wait` 로 위임한다
/// (`child_index` → 호스트 `child` 매핑).
pub(crate) fn handle_wait(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let child_index = require_child_index(params)?;
    let mut wp = forward(params, &["surface"]);
    wp.insert("child".into(), json!(child_index));
    host_call(host, "terminal.wait", Value::Object(wp))
}

/// `claude.tell` 의 자동 wait chain 이 호출하는 mirror 메서드. 입력 `surface_id`
/// (= child surface id) 하나를 호스트 `terminal.wait` 의 surface-mode 로 위임.
pub(crate) fn handle_wait_by_surface(
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let sid = require_surface_id(params)?;
    host_call(host, "terminal.wait", json!({ "surface": sid }))
}

/// `claude.wait_any` 의 입력 파라미터 파싱. host IPC 없이 단위 테스트 가능.
///
/// `--children "1, 2, 3"` 같은 공백 포함 입력도 trim 후 parse — 잘못된 토큰은
/// silent drop 되므로 모든 토큰이 invalid 하거나 empty 면 빈 결과 → invalid_params.
pub(crate) fn parse_wait_any_params(params: &Value) -> Result<(u32, Vec<u32>), IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let children_str = params
        .get("children")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            IpcMethodError::invalid_params("Missing 'children' parameter (comma-separated indices)")
        })?;
    let children: Vec<u32> = children_str
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .collect();
    if children.is_empty() {
        return Err(IpcMethodError::invalid_params(
            "'children' must be a non-empty comma-separated list of indices",
        ));
    }
    Ok((parent_surface_id, children))
}

/// 여러 child 중 *먼저* idle / needs_input / exited 가 되는 것을 즉시 깨운다.
/// 입력 children 순서를 보존하며 각 child 를 호스트 `terminal.wait` 로 조회한다 —
/// 첫 terminal 을 발견하면 즉시 반환, 모두 active 면 `{"state":"pending"}`.
/// 호스트가 등록되지 않은 child 를 invalid_params 로 거부하면 그 자리를 terminal
/// `exited` 로 취급한다(옛 wait-any 의 미발급/정리된 index 동작 보존).
pub(crate) fn handle_wait_any(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let (parent_surface_id, children) = parse_wait_any_params(params)?;
    for child_index in children {
        let state = match host.call(
            "terminal.wait",
            json!({ "surface": parent_surface_id, "child": child_index }),
        ) {
            Ok(v) => v
                .get("state")
                .and_then(|s| s.as_str())
                .unwrap_or("active")
                .to_string(),
            // 등록되지 않은/정리된 child → terminal exited (옛 wait-any 동작 보존).
            Err(_) => "exited".to_string(),
        };
        if state == "exited" || state == "idle" || state == "needs_input" {
            return Ok(json!({ "state": state, "child_index": child_index }));
        }
    }
    Ok(json!({ "state": "pending" }))
}

/// 자식 Claude 를 종료한다 — 호스트 `terminal.kill` 로 위임(surface.close +
/// soft 점유 해제 + registry 제거). `child_index` → 호스트 `child` 매핑.
pub(crate) fn handle_kill(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let child_index = require_child_index(params)?;
    let mut kp = forward(params, &["surface"]);
    kp.insert("child".into(), json!(child_index));
    // 호스트 terminal.kill 성공 시 { killed_surface_id, child_index } 반환. claude
    // CLI 는 기존에 { killed: true } 를 기대하므로 성공을 그 shape 으로 변환한다.
    let resp = host_call(host, "terminal.kill", Value::Object(kp))?;
    let _ = resp; // 호스트가 close 실패 시 이미 error 를 돌려주므로 여기 도달=성공.
    Ok(json!({ "killed": true }))
}

/// 부모의 모든(또는 role 필터된) 자식에 텍스트를 broadcast — 호스트
/// `terminal.broadcast` 로 위임.
pub(crate) fn handle_broadcast(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'text' parameter"))?;
    let mut bp = forward(params, &["surface", "role"]);
    bp.insert("text".into(), json!(text));
    host_call(host, "terminal.broadcast", Value::Object(bp))
}

/// 자식 Claude 에 메시지를 보낸다 — 호스트 `terminal.tell` 로 위임. 개행/제출
/// 규칙(단일라인 평문 / 멀티라인 bracketed paste + 별도 `\r`)은 호스트가 동일하게
/// 처리하므로 본문 포맷을 재구현하지 않는다.
pub(crate) fn handle_tell(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface_id(params)?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'message' parameter"))?;
    host_call(
        host,
        "terminal.tell",
        json!({ "surface": surface_id, "text": message }),
    )
}

/// 새 workspace 를 만들고 그 안에서 claude 를 기동한다. child 가 아니라 top-level
/// 이므로 호스트 child registry 에 등록하지 않는다(launch 는 05 범위 밖 특화 잔류).
/// error scanner 에 그 surface 를 등록한다.
pub(crate) fn handle_launch(
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("claude")
        .to_string();
    let directory = params
        .get("directory")
        .and_then(|v| v.as_str())
        .map(String::from);
    let task = params
        .get("task")
        .and_then(|v| v.as_str())
        .map(String::from);

    // cwd 는 CLI 가 미리 absolute path 로 정규화 + 검증해 전달 (path_kind hint).
    // 호스트 workspace.create 가 직접 PTY 의 working_dir 로 사용 → `cd` echo trick 불필요.
    let mut ws_params = json!({
        "type": "terminal",
        "name": workspace_name,
    });
    if let Some(dir) = directory.as_deref() {
        ws_params["cwd"] = Value::String(dir.to_string());
    }
    let ws_resp = host
        .call("workspace.create", ws_params)
        .map_err(IpcMethodError::from)?;

    let workspace_id = ws_resp
        .get("id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("workspace.create returned no 'id'"))?;
    let surface_id = ws_resp
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if let Some(sid) = surface_id {
        let cmd = build_launch_command(task.as_deref());
        if let Err(e) = host.call(
            "surface.send",
            json!({ "surface_id": sid, "text": format!("{cmd}\r") }),
        ) {
            tracing::warn!("surface.send (launch) failed: {e}");
        }

        if let Ok(mut s) = scanner.lock() {
            s.enable(sid);
        }
    }

    Ok(json!({
        "workspace_id": workspace_id,
        "workspace_name": workspace_name,
        "surface_id": surface_id,
    }))
}

/// `claude` 또는 `claude --task <escaped>`.
pub(crate) fn build_launch_command(task: Option<&str>) -> String {
    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {escaped}"));
    }
    cmd
}

/// 자식 surface 의 PTY 를 갈아끼우고 claude 를 재시작한다. registry 조작(PTY 교체/
/// Ctrl-C + metadata 갱신 + idle 초기화)은 호스트 `terminal.respawn` 으로 위임하고,
/// claude 특화 기동 명령만 그 위에 재전송한다. `child_index` → 호스트 `child` 매핑.
pub(crate) fn handle_respawn(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let child_index = require_child_index(params)?;
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    // 1) 호스트 registry 위임(command 미전송): cwd 있으면 PTY 교체, 없으면 Ctrl-C.
    //    role/nickname/cwd 갱신 + idle 초기화까지 호스트가 수행.
    let mut rp = forward(params, &["surface", "cwd", "role", "nickname"]);
    rp.insert("child".into(), json!(child_index));
    let resp = host_call(host, "terminal.respawn", Value::Object(rp))?;
    let child_surface_id = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "terminal.respawn response missing 'child_surface_id': {resp}"
            ))
        })?;

    // 2) claude 특화 기동 명령 재전송.
    start_claude_in_surface(host, child_surface_id, prompt.as_deref());

    Ok(json!({
        "child_surface_id": child_surface_id,
        "child_index": child_index,
        "parent_surface_id": parent_surface_id,
    }))
}

/// 자식 surface 에서 claude 를 기동한다. surface_id 를 박은 inline env prefix 를
/// 항상 붙인다:
/// - `TASTY_SURFACE_ID={surface_id}` — 자식 셸이 `tasty claude hook` 을 발사할 때
///   자기 위치 식별 (없으면 hook 이 silent skip → idle/needs_input 미갱신).
/// - `TASTY_AGENT_ID=claude_s<surface_id>` — 관측/비용 agent 식별.
/// - `TASTY_SESSION_TOKEN=<hex>` — 신원 검증 토큰(발급 실패 시 생략).
pub(crate) fn start_claude_in_surface(host: &HostHandle, surface_id: u32, prompt: Option<&str>) {
    let agent_id = format!("claude_s{surface_id}");
    let session_token = issue_session_token(host, &agent_id);
    let agent_prefix = match session_token {
        Some(tok) => format!(
            "TASTY_SURFACE_ID={surface_id} TASTY_AGENT_ID={agent_id} TASTY_SESSION_TOKEN={tok} "
        ),
        None => format!("TASTY_SURFACE_ID={surface_id} TASTY_AGENT_ID={agent_id} "),
    };

    if let Some(p) = prompt {
        let prompt_path = std::env::temp_dir().join(format!("tasty-prompt-{}.txt", surface_id));
        if let Err(e) = std::fs::write(&prompt_path, p) {
            tracing::warn!("Failed to write prompt file: {e}");
        }
        if let Err(e) = host.call(
            "surface.send",
            json!({
                "surface_id": surface_id,
                "text": format!("{agent_prefix}claude \"$(cat '{}')\"\r", prompt_path.display()),
            }),
        ) {
            tracing::warn!("surface.send (claude with prompt) failed: {e}");
        }
    } else if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": format!("{agent_prefix}claude\r") }),
    ) {
        tracing::warn!("surface.send (claude) failed: {e}");
    }
}

/// 자식 Claude 에 발급할 SessionToken 을 호스트에서 가져온다. 부모(claude plugin)의
/// 권한 부분집합만 발급되며, 발급 실패는 치명적이지 않으므로 `Option` 반환.
pub(crate) fn issue_session_token(host: &HostHandle, agent_id: &str) -> Option<String> {
    let resp = match host.call(
        "session.issue",
        json!({
            "agent_id": agent_id,
            "permissions": [
                "surface.read",
                "surface.write",
                "terminal.write",
                "terminal.read",
                "notification",
                "telemetry",
                "agent",
            ],
        }),
    ) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("session.issue failed for child {agent_id}: {e}");
            return None;
        }
    };
    let token = resp.get("token").and_then(|v| v.as_str()).map(String::from);
    if token.is_none() {
        tracing::warn!("session.issue returned no 'token' field for child {agent_id}");
    }
    token
}

/// 자식 Claude 를 spawn 한다 — 호스트 `terminal.spawn` 으로 registry 등록 + soft
/// 점유 + tab 생성(command 미전송)한 뒤, 반환된 surface_id 에 claude 특화 기동
/// 명령을 전송한다(2단계 spawn). 호스트가 index/tab-name/pane/occupancy 를 소유.
pub(crate) fn handle_spawn(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    // 1) 호스트 registry 에 등록 + 점유 + tab 생성. workspace required.
    let mut sp = forward(params, &["workspace", "pane", "cwd", "role", "nickname"]);
    sp.insert("parent".into(), json!(parent_surface_id));
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child_surface_id = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "terminal.spawn response missing 'child_surface_id': {resp}"
            ))
        })?;

    // 2) claude 특화 기동 명령 전송(session token + surface_id inline env 필요).
    start_claude_in_surface(host, child_surface_id, prompt.as_deref());

    // claude CLI/auto_wait 는 응답에 parent_surface_id 를 기대한다(호스트 응답엔
    // 없으므로 caller surface 로 채운다). 나머지 필드(child_surface_id/child_index/
    // pane_id/workspace_id)는 호스트 응답 그대로.
    let mut out = resp;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("parent_surface_id".into(), json!(parent_surface_id));
    }

    // 3) child 개수 임계치 경고(soft) — spawn 자체를 막지 않는다.
    if let Some(warning) = compute_spawn_warning(host, parent_surface_id) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("warning".into(), json!(warning));
        }
    }

    Ok(out)
}

const DEFAULT_SPAWN_CHILD_WARN_THRESHOLD: f64 = 6.0;

/// spawn 직후 parent 의 현재 child 목록/상태를 재조회해 임계치 초과 여부를 판단한다.
/// host 호출 실패는 경고 생략으로 처리한다(soft 경고이므로 spawn 성공을 막지 않음).
///
/// 여기서 부르는 건 claude 특화 remap 된 `claude.children`(필드명 `child_surface_id`)이
/// 아니라 **원본** `terminal.children`(필드명 `surface_id`) — `index`/`state` 필드명은
/// 양쪽 shape 모두 동일하므로 아래 파싱 코드는 원본 응답에 그대로 맞는다.
fn compute_spawn_warning(host: &HostHandle, parent_surface_id: u32) -> Option<String> {
    let children_resp = host
        .call("terminal.children", json!({ "surface": parent_surface_id }))
        .ok()?;
    let children = children_resp.get("children")?.as_array()?;
    let total = children.len();
    let idle_indices: Vec<u64> = children
        .iter()
        .filter(|c| c.get("state").and_then(|s| s.as_str()) == Some("idle"))
        .filter_map(|c| c.get("index").and_then(|i| i.as_u64()))
        .collect();

    let threshold = host
        .call(
            "settings.get_plugin_setting",
            json!({ "storage_key": "spawn_child_warn_threshold" }),
        )
        .ok()
        .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_SPAWN_CHILD_WARN_THRESHOLD);

    build_spawn_warning(total, &idle_indices, threshold)
}

/// 순수 함수 — host 호출 없음. 단위 테스트 대상.
fn build_spawn_warning(total: usize, idle_indices: &[u64], threshold: f64) -> Option<String> {
    if (total as f64) <= threshold {
        return None;
    }
    let mut msg = format!(
        "{total} child instances are currently spawned under this parent (warning threshold: {threshold}). Consider checking for leaked children."
    );
    if !idle_indices.is_empty() {
        let idle_list = idle_indices
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        msg.push_str(&format!(
            " Idle children at index [{idle_list}] have already finished their work — consider `respawn` instead of spawning a new one."
        ));
    }
    Some(msg)
}

/// 자식 surface 의 parent 를 조회 — 호스트 `terminal.parent` 로 위임.
pub(crate) fn handle_parent(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let surface = require_surface_id(params)?;
    host_call(host, "terminal.parent", json!({ "surface": surface }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_spawn_warning_none_below_threshold() {
        assert_eq!(build_spawn_warning(3, &[], 6.0), None);
    }

    #[test]
    fn build_spawn_warning_above_threshold_lists_idle_and_mentions_respawn() {
        let w = build_spawn_warning(7, &[2, 5], 6.0).unwrap();
        assert!(w.contains("respawn"));
        assert!(w.contains('2') && w.contains('5'));
    }

    #[test]
    fn build_spawn_warning_above_threshold_no_idle_has_no_respawn_word() {
        let w = build_spawn_warning(7, &[], 6.0).unwrap();
        assert!(!w.contains("respawn"));
    }

    #[test]
    fn build_spawn_warning_respects_custom_threshold() {
        assert_eq!(build_spawn_warning(3, &[], 6.0), None);
        assert!(build_spawn_warning(4, &[], 3.0).is_some());
    }

    #[test]
    fn build_launch_command_no_task() {
        assert_eq!(build_launch_command(None), "claude");
    }

    #[test]
    fn build_launch_command_with_simple_task() {
        assert_eq!(build_launch_command(Some("fix")), "claude --task fix");
    }

    #[test]
    fn build_launch_command_with_spaces_gets_escaped() {
        let out = build_launch_command(Some("fix the bug"));
        assert!(out.starts_with("claude --task "), "prefix wrong: {out}");
        assert!(out.contains("fix the bug"), "task body missing: {out}");
        assert_ne!(out, "claude --task fix the bug", "must be escaped");
    }

    #[test]
    fn parse_wait_any_params_errors_on_empty_children() {
        let err = parse_wait_any_params(&json!({
            "surface_id": 10,
            "children": "",
        }))
        .unwrap_err();
        assert_eq!(err.code, -32602, "expected invalid_params, got {err:?}");
    }

    #[test]
    fn parse_wait_any_params_parses_spaced_list() {
        let (parent, children) =
            parse_wait_any_params(&json!({ "surface_id": 10, "children": "1, 2 ,3" })).unwrap();
        assert_eq!(parent, 10);
        assert_eq!(children, vec![1, 2, 3]);
    }

    #[test]
    fn require_child_index_missing_is_invalid_params() {
        let err = require_child_index(&json!({ "surface_id": 1 })).unwrap_err();
        assert_eq!(err.code, -32602);
    }
}
