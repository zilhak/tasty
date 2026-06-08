//! `tasty-claude` 의 IPC handler fn 들 — 외부 plugin SDK 진입점.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::error_scan::ErrorScanner;
use crate::state::{ChildEntry, ClaudeState};

pub(crate) fn require_surface_id(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'surface_id' parameter"))
}

pub(crate) fn handle_set_idle_state(
    state: &mut ClaudeState,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("No focused surface"))?;
    let idle = params
        .get("idle")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'idle' parameter (bool)"))?;
    state.set_idle(surface_id, idle);
    state.save();
    Ok(json!({ "ok": true }))
}

pub(crate) fn handle_set_needs_input(
    state: &mut ClaudeState,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("No focused surface"))?;
    let needs_input = params
        .get("needs_input")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'needs_input' parameter (bool)"))?;
    state.set_needs_input(surface_id, needs_input);
    state.save();
    Ok(json!({ "ok": true }))
}

/// 호스트 `handle_claude_children` 1:1 이주. 자식 목록을 ClaudeState에서 읽고,
/// 각 자식의 PTY 전경 프로세스는 `surface.foreground_process` IPC로 조회한다.
/// IPC 실패는 무시 (host가 terminal을 못 찾으면 필드를 안 넣고 응답하던 동작과
/// 동일하게 None이 들어가 그 키들은 생략됨).
pub(crate) fn handle_children(
    state: &ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let mut entries = children_base_entries(state, parent_surface_id);
    for entry in &mut entries {
        let sid = entry
            .get("child_surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let Some(sid) = sid else { continue };
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

/// `handle_children`의 순수 부분: state만으로 결정 가능한 baseline entry 리스트.
/// 호스트 응답의 foreground_process / foreground_pid는 여기 포함되지 않는다.
pub(crate) fn children_base_entries(state: &ClaudeState, parent_surface_id: u32) -> Vec<Value> {
    state
        .list_children(parent_surface_id)
        .iter()
        .map(|c| {
            json!({
                "child_surface_id": c.child_surface_id,
                "index": c.index,
                "cwd": c.cwd,
                "role": c.role,
                "nickname": c.nickname,
                "state": state.state_of(c.child_surface_id),
            })
        })
        .collect()
}

/// 호스트 `handle_claude_wait` 1:1 이주. 한 번의 호출은 1회 상태 스냅샷이며,
/// CLI 측 polling(`run_claude_wait`)이 idle/needs_input/exited 도달까지 반복
/// 호출한다. 본 함수는 그 polling tick 1개를 처리한다.
pub(crate) fn handle_wait(
    state: &ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let child_index = params
        .get("child_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'child_index' parameter"))?;

    let decision = wait_decide(state, parent_surface_id, child_index);
    let response_state = match decision {
        WaitDecision::Exited => "exited",
        WaitDecision::CheckExistence(child_surface_id) => {
            let exists = host
                .call("surface.locate", json!({ "surface_id": child_surface_id }))
                .ok()
                .and_then(|v| v.get("exists").and_then(|e| e.as_bool()))
                .unwrap_or(false);
            if !exists {
                "exited"
            } else {
                state.state_of(child_surface_id)
            }
        }
    };
    Ok(json!({ "state": response_state }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitDecision {
    /// child가 ClaudeState에 없다 → 즉시 "exited".
    Exited,
    /// child가 state에 있다 → 호스트 트리에 surface가 살아있는지 확인 필요.
    /// 살아있으면 `state.state_of(child_surface_id)`, 죽었으면 "exited".
    CheckExistence(u32),
}

/// state만으로 결정 가능한 wait 분기. host IPC 없이 단위 테스트 가능.
pub(crate) fn wait_decide(
    state: &ClaudeState,
    parent_surface_id: u32,
    child_index: u32,
) -> WaitDecision {
    match state.find_child(parent_surface_id, child_index) {
        Some(c) => WaitDecision::CheckExistence(c.child_surface_id),
        None => WaitDecision::Exited,
    }
}

/// `wait-any` 의 *state-only* 결정. 입력 children 순서를 보존하며 각 child 의
/// `WaitDecision` (`Exited` 또는 `CheckExistence`) 을 수집한다.
///
/// host IPC 없이 단위 테스트 가능. caller (`handle_wait_any`) 는 결과 Vec 을
/// 입력 순서대로 iterate 하여 첫 *terminal* 을 결정한다 — `Exited` 면 즉시
/// terminal, `CheckExistence` 는 host 확인 후 *non-active* 이면 terminal.
///
/// **순서 보존이 중요한 이유**: G.F-Q2 "순서대로 첫 매치" 정책. mixed
/// early-return (Exited 즉시 반환 + CheckExistence 후보 누적) 은
/// children=[a:CheckExistence→idle, b:Exited, c:CheckExistence→active] 같은
/// 케이스에서 b 의 exited 가 a 의 idle 을 가로채 정책을 위반한다. 따라서
/// 본 함수는 *모든 children 의 WaitDecision* 을 단순 collect 하고, 우선순위
/// 평가는 caller 에게 넘긴다.
pub(crate) fn wait_any_decide(
    state: &ClaudeState,
    parent_surface_id: u32,
    children: &[u32],
) -> Vec<(u32, WaitDecision)> {
    children
        .iter()
        .map(|&i| (i, wait_decide(state, parent_surface_id, i)))
        .collect()
}

/// `claude.wait_any` 의 입력 파라미터 파싱. host IPC 없이 단위 테스트 가능.
///
/// `--children "1, 2, 3"` 같은 공백 포함 입력도 trim 후 parse — 잘못된 토큰
/// 은 silent drop 되므로 모든 토큰이 invalid 한 경우와 empty 입력은 둘 다
/// 빈 결과 → invalid_params 에러 (G.F-Q3). 정상 케이스는 (parent_surface_id,
/// children Vec) 반환.
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

/// `wait_any_decide` 결과 + host/state 콜백을 받아 응답 JSON 을 구성하는 순수
/// 함수. 입력 children 순서대로 한 child 씩 평가 — 첫 terminal 을 발견하면
/// 즉시 반환, 모두 active 면 `{"state":"pending"}` 반환 (child_index 키 없음,
/// R10).
///
/// terminal 판정:
/// - `WaitDecision::Exited` (state 에 없음): 즉시 `exited`.
/// - `WaitDecision::CheckExistence(sid)`: `exists_fn(sid) == false` → `exited`,
///   `state_of_fn(sid) ∈ {"idle","needs_input"}` → 그 state.
/// - 그 외 (`active`) 는 다음 child 로 진행.
pub(crate) fn wait_any_response<F, G>(
    decisions: &[(u32, WaitDecision)],
    mut exists_fn: F,
    mut state_of_fn: G,
) -> Value
where
    F: FnMut(u32) -> bool,
    G: FnMut(u32) -> &'static str,
{
    for &(child_index, decision) in decisions {
        match decision {
            WaitDecision::Exited => {
                return json!({ "state": "exited", "child_index": child_index });
            }
            WaitDecision::CheckExistence(child_surface_id) => {
                if !exists_fn(child_surface_id) {
                    return json!({ "state": "exited", "child_index": child_index });
                }
                let s = state_of_fn(child_surface_id);
                if s == "idle" || s == "needs_input" {
                    return json!({ "state": s, "child_index": child_index });
                }
            }
        }
    }
    json!({ "state": "pending" })
}

/// `claude.wait_any` IPC 핸들러.
///
/// 여러 child 중 *먼저* idle / needs_input / exited 가 되는 것을 즉시
/// 깨운다. CLI 측 `run_dynamic_client_polling` 이 매 tick (interval_ms)
/// 마다 본 핸들러를 호출하며, 응답의 `state` 가 terminal_states 매치되면
/// polling 종료.
pub(crate) fn handle_wait_any(
    state: &ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let (parent_surface_id, children) = parse_wait_any_params(params)?;
    let decisions = wait_any_decide(state, parent_surface_id, &children);
    Ok(wait_any_response(
        &decisions,
        |sid| {
            host.call("surface.locate", json!({ "surface_id": sid }))
                .ok()
                .and_then(|v| v.get("exists").and_then(|e| e.as_bool()))
                .unwrap_or(false)
        },
        |sid| state.state_of(sid),
    ))
}

/// 호스트 `handle_claude_kill` 1:1 이주.
/// 1. ClaudeState에서 (parent_surface_id, child_index) → child_surface_id 해석
/// 2. `surface.locate` IPC로 pane_id 조회 (호스트의 `find_pane_for_surface`)
/// 3. `pane.close` IPC로 pane 제거 (호스트의 `close_pane_by_id` + 부수 효과)
/// 4. 성공 시 plugin state 정리 (unregister_child + mark_parent_closed)
pub(crate) fn handle_kill(
    state: &mut ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let child_index = params
        .get("child_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'child_index' parameter"))?;

    let child_surface_id = state
        .find_child(parent_surface_id, child_index)
        .map(|c| c.child_surface_id)
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&format!(
                "Child index {} not found for parent {}",
                child_index, parent_surface_id
            ))
        })?;

    let locate = host
        .call("surface.locate", json!({ "surface_id": child_surface_id }))
        .map_err(|e| IpcMethodError::new(format!("surface.locate failed: {e}")))?;
    let pane_id = locate
        .get("pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&format!("Surface {} not found", child_surface_id))
        })?;

    let close_resp = host
        .call("pane.close", json!({ "pane_id": pane_id }))
        .map_err(|e| IpcMethodError::new(format!("pane.close failed: {e}")))?;
    let killed = close_resp
        .get("closed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if killed {
        kill_finalize(state, child_surface_id);
    }

    Ok(json!({ "killed": killed }))
}

/// pane.close 성공 후의 state mutation. 호스트와 동일하게 child_surface_id를
/// `mark_parent_closed`에도 넘긴다 — 그 자식이 또 다른 parent를 가진 nested
/// claude 시나리오에서만 의미가 있고, 그렇지 않으면 no-op.
pub(crate) fn kill_finalize(state: &mut ClaudeState, child_surface_id: u32) {
    state.unregister_child(child_surface_id);
    state.mark_parent_closed(child_surface_id);
    state.save();
}

/// 호스트 `handle_claude_broadcast` 1:1 이주.
///
/// **주의 — 미세한 동작 차이**: 호스트는 `find_terminal_by_id_mut`로 직접
/// terminal에 송신하지만, 플러그인은 `surface.send` IPC를 거치므로 deferred
/// surface에 대해 PTY가 자동 초기화된다. 일상 시나리오(spawn → broadcast)에서는
/// 차이가 관측되지 않는다.
pub(crate) fn handle_broadcast(
    state: &ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'text' parameter"))?;
    let role_filter = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(String::from);

    let child_ids = broadcast_targets(state, parent_surface_id, role_filter.as_deref());

    let mut sent_count = 0usize;
    for sid in &child_ids {
        if host
            .call("surface.send", json!({ "surface_id": sid, "text": text }))
            .is_ok()
        {
            sent_count += 1;
        }
    }

    Ok(json!({
        "sent_count": sent_count,
        "children": child_ids,
    }))
}

/// state만으로 결정 가능한 broadcast 대상 child_surface_id 목록.
/// role_filter=Some이면 그 role을 가진 자식만, None이면 전체.
pub(crate) fn broadcast_targets(
    state: &ClaudeState,
    parent_surface_id: u32,
    role_filter: Option<&str>,
) -> Vec<u32> {
    state
        .list_children(parent_surface_id)
        .iter()
        .filter(|c| match role_filter {
            Some(r) => c.role.as_deref() == Some(r),
            None => true,
        })
        .map(|c| c.child_surface_id)
        .collect()
}

/// 호스트 `handle_claude_tell` 1:1 이주.
///
/// Claude Code의 handleEnter 로직과 맞물리는 PTY 시퀀스를 만들어 surface.send로
/// 보낸다 — 줄바꿈은 `\` + `\r` (newline 삽입), 마지막 `\r`이 submit.
///
/// **주의 — 미세한 동작 차이**: `handle_broadcast`와 동일하게 surface.send를
/// 거치므로 deferred surface는 auto-init된다.
pub(crate) fn handle_tell(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface_id(params)?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'message' parameter"))?;
    let pty_text = build_tell_pty_text(message);

    let resp = host
        .call(
            "surface.send",
            json!({ "surface_id": surface_id, "text": pty_text }),
        )
        .map_err(IpcMethodError::from)?;

    // surface.send는 `{sent: true, surface_id}` 응답. 호스트 claude.tell과 동일
    // 필드 구성이므로 그대로 반환.
    Ok(resp)
}

/// 호스트 `handle_claude_launch` 1:1 이주.
///
/// 1. `workspace.create { type: "terminal", name }`로 새 워크스페이스 + 초기
///    터미널 생성.
/// 2. 디렉터리 인자가 있으면 `cd <escaped>\r`을 PTY로 송신 (호스트와 동일하게
///    workspace.create의 cwd가 아니라 PTY cd 사용 — 사용자가 cd 명령 echo를
///    볼 수 있는 동작 보존).
/// 3. `claude` (+ optional `--task <escaped>`)을 PTY로 송신.
/// 4. plugin 자체 error scanner에 surface 등록.
///
/// 호스트가 호출하던 `terminal.set_output_scan_mark()`는 plugin이 가진 IPC
/// (`surface.read_since_mark`)와 서로 다른 mark이므로 1:1 대응이 없다. error_scan
/// 모듈은 `surface.read_since_mark`로 읽고 200자 dedupe로 중복 fire를 막으므로
/// 누락이 아니라 false positive 위험이 미세하게 늘 뿐이며, 정규식이 Claude API
/// 응답에 매우 특이적이라 실측 영향은 거의 없다.
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

/// `claude` 또는 `claude --task <escaped>`. host 측 launch와 동일한 escape 사용.
pub(crate) fn build_launch_command(task: Option<&str>) -> String {
    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {escaped}"));
    }
    cmd
}

/// 호스트 `handle_claude_respawn` 1:1 이주. 자식 surface의 PTY를 새 프로세스로
/// 교체하고 `claude` 명령을 재송신한다.
///
/// 호스트 코드와 동일한 절차:
/// 1. (parent_surface_id, child_index) → child_surface_id 해석.
/// 2. `surface.respawn_terminal` IPC로 PTY 갈아끼움 — working_dir는 항상 None
///    (호스트도 그렇게 하고 PTY로 `cd` echo).
/// 3. 새 metadata(cwd/role/nickname)가 주어진 경우에만 child entry 업데이트.
/// 4. cwd cd → prompt가 있으면 prompt 파일 + `claude "$(cat ...)"\r`,
///    아니면 `claude\r`.
pub(crate) fn handle_respawn(
    state: &mut ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let child_index = params
        .get("child_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'child_index' parameter"))?;
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let existing_child = state
        .find_child(parent_surface_id, child_index)
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&format!(
                "Child index {} not found for parent {}",
                child_index, parent_surface_id
            ))
        })?;
    let child_surface_id = existing_child.child_surface_id;
    // caller 가 cwd 를 안 줬으면 기존 child 의 cwd 를 재사용 — 같은 디렉토리에서
    // 재시작. 기존도 없으면 호스트 inherit (None).
    let effective_cwd = cwd.clone().or_else(|| existing_child.cwd.clone());

    let mut respawn_params = json!({ "surface_id": child_surface_id });
    if let Some(c) = effective_cwd.as_deref() {
        respawn_params["cwd"] = Value::String(c.to_string());
    }
    host.call("surface.respawn_terminal", respawn_params)
        .map_err(IpcMethodError::from)?;

    let updated = update_child_metadata(
        state,
        parent_surface_id,
        child_index,
        cwd.as_deref(),
        role.as_deref(),
        nickname.as_deref(),
    );
    if updated {
        state.save();
    }

    start_claude_in_surface(host, child_surface_id, prompt.as_deref());

    Ok(json!({
        "child_surface_id": child_surface_id,
        "child_index": child_index,
        "parent_surface_id": parent_surface_id,
    }))
}

/// host code line 591-602의 metadata 부분 갱신 로직. None은 기존 값을 보존.
/// 반환: 한 필드라도 갱신됐는지 여부 (false면 save 호출 생략 가능).
pub(crate) fn update_child_metadata(
    state: &mut ClaudeState,
    parent_surface_id: u32,
    child_index: u32,
    cwd: Option<&str>,
    role: Option<&str>,
    nickname: Option<&str>,
) -> bool {
    let any = cwd.is_some() || role.is_some() || nickname.is_some();
    if !any {
        return false;
    }
    state.update_child(parent_surface_id, child_index, |entry| {
        if let Some(v) = cwd {
            entry.cwd = Some(v.to_string());
        }
        if let Some(v) = role {
            entry.role = Some(v.to_string());
        }
        if let Some(v) = nickname {
            entry.nickname = Some(v.to_string());
        }
    })
}

/// 호스트 `start_claude_in_surface` 1:1 이주. 인자는 동일하나 IPC 경유.
///
/// inline env prefix:
/// - `TASTY_AGENT_ID=claude_s<surface_id>` — Phase 4 (관측/비용) agent 식별.
///   shell history 에 echo 되지만 사용자가 직접 입력했을 때와 동일.
/// - `TASTY_SESSION_TOKEN=<hex>` — Phase 6.2 신원 검증. `session.issue` 로
///   호스트에서 발급받은 토큰. 자식이 IPC envelope 에 함께 보내면 호스트가
///   `CallerContext::Agent` 로 분기. 발급 실패 시 token prefix 만 생략 — 자식은
///   계속 `TASTY_AGENT_ID` 로 self-reporting 은 가능하나, agent 권한 게이트가
///   필요한 메서드(agent.*/session.* 등)는 호출 불가.
pub(crate) fn start_claude_in_surface(host: &HostHandle, surface_id: u32, prompt: Option<&str>) {
    // cwd 는 split / workspace.create / surface.respawn_terminal 호출 시 호스트에
    // 직접 전달되어 PTY working_dir 로 적용된다. 더 이상 `cd <path>\r` echo 가
    // 필요 없다 (PTY 자체 cwd 기준 cd → silent fail 사고 제거).
    let agent_id = format!("claude_s{surface_id}");
    let session_token = issue_session_token(host, &agent_id);
    // TASTY_SURFACE_ID 는 자식 셸이 `tasty claude hook` 명령을 발사할 때 자기
    // 위치 식별에 쓰인다. ~/.claude/settings.json 의 hook entry 는
    // `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook stop || true` 형태이므로
    // 이 env 가 없으면 silent skip — Stop/Notification hook 전체가 사라진다.
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

/// 자식 Claude 에 발급할 SessionToken 을 호스트에서 가져온다.
///
/// 매니페스트 `permissions` 의 부분집합만 자식에게 줄 수 있다. 현재는 자식이
/// Claude Code 의 정상 동선을 그대로 흉내내야 하므로 부모(claude plugin)의 권한을
/// 그대로 상속한다 — 자식이 plugin 자체와 동일한 surface/terminal/fs 조작이 필요.
/// 토큰 발급 실패는 치명적이지 않다 (Phase 6.2 권한 게이트 적용 메서드는 거부될
/// 뿐, 기존 흐름은 유지). 그래서 `Option` 반환.
pub(crate) fn issue_session_token(host: &HostHandle, agent_id: &str) -> Option<String> {
    let resp = match host.call(
        "session.issue",
        json!({
            "agent_id": agent_id,
            // 자식이 사용할 권한. 호스트는 caller(claude plugin)의 권한 셋에
            // 포함된 토큰만 발급한다 (escalation 방지). manifest 의 권한과
            // 정확히 같지는 않아도 되며, 자식이 실제로 필요한 메서드를 위한
            // 토큰만 명시.
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

/// 호스트 `handle_claude_spawn` 1:1 이주.
///
/// parent surface가 사는 workspace에 자동 관리되는 "spawn pane" 안에 2x2 grid로
/// 새 자식 surface를 배치하고 그 안에서 claude를 실행한다. 한 탭에 4개를 채우면
/// 같은 spawn pane에 새 탭을 만든다. 사용자가 spawn pane을 닫았으면 다음 호출
/// 시 자동으로 새 spawn pane을 만든다.
pub(crate) fn handle_spawn(
    state: &mut ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let workspace_param = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required '--workspace' parameter"))?
        .to_string();
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let pane_override = params
        .get("pane")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let ws_id = resolve_workspace_id(host, &workspace_param)?.ok_or_else(|| {
        IpcMethodError::invalid_params(&format!("Workspace '{}' not found", workspace_param))
    })?;

    // 대상 pane: --pane 지정 시 그 pane, 아니면 workspace 의 첫 pane.
    let pane_id = match pane_override {
        Some(p) => p,
        None => first_pane_in_workspace(host, ws_id)?,
    };

    // child index 를 먼저 확보해 탭 이름을 child{N} 으로 고정 생성한다.
    let child_index = state.next_child_index(parent_surface_id);
    let tab_name = format!("child{child_index}");
    let mut tab_params = json!({
        "pane_id": pane_id,
        "type": "terminal",
        "name": tab_name,
    });
    if let Some(c) = cwd.as_deref() {
        tab_params["cwd"] = Value::String(c.to_string());
    }
    let tab_resp = host
        .call("tab.create", tab_params)
        .map_err(IpcMethodError::from)?;
    let child_surface_id = tab_resp
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("tab.create returned no 'surface_id'"))?;

    state.register_child(
        parent_surface_id,
        ChildEntry {
            child_surface_id,
            index: child_index,
            cwd,
            role,
            nickname,
        },
    );
    state.save();

    start_claude_in_surface(host, child_surface_id, prompt.as_deref());

    Ok(json!({
        "child_surface_id": child_surface_id,
        "child_index": child_index,
        "parent_surface_id": parent_surface_id,
        "pane_id": pane_id,
        "workspace_id": ws_id,
    }))
}

/// 호스트 `caller_surface_id` 1:1. plugin IPC ctx.params에 같은 키가 들어온다.
pub(crate) fn caller_surface_id(params: &Value) -> Option<u32> {
    params
        .get("caller_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
}

/// 호스트 `resolve_workspace` 1:1. target이 숫자면 id, 아니면 name으로 매칭.
pub(crate) fn resolve_workspace_id(
    host: &HostHandle,
    target: &str,
) -> Result<Option<u32>, IpcMethodError> {
    let ws_list = host
        .call("workspace.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let arr = ws_list
        .as_array()
        .ok_or_else(|| IpcMethodError::new("workspace.list returned non-array"))?;
    if let Ok(target_id) = target.parse::<u32>() {
        for w in arr {
            if w.get("id").and_then(|v| v.as_u64()) == Some(target_id as u64) {
                return Ok(Some(target_id));
            }
        }
    }
    for w in arr {
        if w.get("name").and_then(|v| v.as_str()) == Some(target)
            && let Some(id) = w.get("id").and_then(|v| v.as_u64())
        {
            return Ok(Some(id as u32));
        }
    }
    Ok(None)
}

/// workspace 내 첫 pane 의 id. spawn 시 `--pane` 미지정의 기본 대상이다.
fn first_pane_in_workspace(host: &HostHandle, ws_id: u32) -> Result<u32, IpcMethodError> {
    let panes = host
        .call("pane.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let arr = panes
        .as_array()
        .ok_or_else(|| IpcMethodError::new("pane.list returned non-array"))?;
    arr.iter()
        .find(|p| p.get("workspace_id").and_then(|v| v.as_u64()) == Some(ws_id as u64))
        .and_then(|p| p.get("id").and_then(|v| v.as_u64()).map(|v| v as u32))
        .ok_or_else(|| IpcMethodError::new(format!("No panes in workspace {ws_id}")))
}

/// 호스트 코드의 PTY 시퀀스 생성 로직을 1:1 옮긴 순수 함수.
/// - 라인 사이: `\` + `\r` (Claude Code에서 newline 삽입)
/// - 마지막 라인이 `\`로 끝나면 ` ` 한 칸을 덧붙여 final `\r`이 submit으로 해석되게
/// - 끝에 `\r` 추가 = submit
pub(crate) fn build_tell_pty_text(message: &str) -> String {
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

pub(crate) fn handle_parent(state: &ClaudeState, params: &Value) -> Result<Value, IpcMethodError> {
    let child_surface_id = require_surface_id(params)?;
    match state.parent_of_child(child_surface_id) {
        Some(parent_id) => {
            let status = if state.is_parent_closed(parent_id) {
                "closed"
            } else {
                "active"
            };
            Ok(json!({
                "parent_surface_id": parent_id,
                "status": status,
            }))
        }
        None => Ok(json!({
            "parent_surface_id": null,
            "status": "none",
        })),
    }
}
