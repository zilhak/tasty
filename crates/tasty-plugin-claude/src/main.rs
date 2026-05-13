//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. Phase 2가 끝나면 호스트 내부에 박혀 있던 Claude Code 통합이
//! 이 plugin으로 일원화되며, codex/aider 등 다른 코딩 에이전트 plugin들과 동등한 1급
//! 확장점 위에서 동작한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod error_scan;
mod hook;
mod install;
mod state;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tasty_plugin_sdk::{
    HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceLifecycleCtx, SurfaceResult,
};

use error_scan::ErrorScanner;
use state::ClaudeState;

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = "0.1.0";

/// PTY 에러 폴링 간격. 호스트 메모리 스캔(O(1))과의 정확도 차이를 좁히기 위해
/// 짧게. 자식 N명에 대해 N IPC/주기지만 N이 10 이하인 일상 시나리오에서는 무시
/// 가능한 부하 (8 calls/sec @ 10 children).
const ERROR_SCAN_INTERVAL: Duration = Duration::from_millis(800);

struct ClaudePlugin {
    state: ClaudeState,
    scanner: Arc<Mutex<ErrorScanner>>,
}

impl ClaudePlugin {
    fn new() -> Self {
        Self {
            state: ClaudeState::load(),
            scanner: Arc::new(Mutex::new(ErrorScanner::new())),
        }
    }
}

impl Plugin for ClaudePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // claude plugin은 자체 surface_kind를 등록하지 않는다 — 자식 Claude 프로세스는
        // 호스트의 일반 terminal surface에서 실행되며, surface 자체는 plugin이 직접
        // 만들지 않는다. 매니페스트에 surface_kinds가 없으므로 이 콜백은 호출되지
        // 않을 것이다.
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // 분기를 작게 cutover-안전 단계로 채워나간다. BUILTINS 미등록 동안엔 모든
        // claude.* 트래픽이 호스트로 가므로 본 분기는 실제로는 단위 테스트로만 검증.
        // 호스트 핸들러 제거 + BUILTINS 등록은 step 04e cutover에서 atomic으로.
        match ctx.method.as_str() {
            "claude.hook" => hook::handle_claude_hook(&mut self.state, &ctx.host, &ctx.params),
            "claude.install" => match install::run_install() {
                Ok(added) => Ok(json!({ "installed": added })),
                Err(e) => Err(IpcMethodError::new(format!("install failed: {e}"))),
            },
            "claude.uninstall" => match install::run_uninstall() {
                Ok(removed) => Ok(json!({ "uninstalled": removed })),
                Err(e) => Err(IpcMethodError::new(format!("uninstall failed: {e}"))),
            },
            // step 04a: plugin 자기 ClaudeState만 보면 응답 가능한 핸들러들.
            "claude.set_idle_state" => handle_set_idle_state(&mut self.state, &ctx.params),
            "claude.set_needs_input" => handle_set_needs_input(&mut self.state, &ctx.params),
            "claude.parent" => handle_parent(&self.state, &ctx.params),
            // step 04b: 호스트 IPC(surface.foreground_process / surface.locate /
            // pane.close)와 ClaudeState를 함께 조합하는 핸들러들.
            "claude.children" => handle_children(&self.state, &ctx.host, &ctx.params),
            "claude.wait" => handle_wait(&self.state, &ctx.host, &ctx.params),
            "claude.kill" => handle_kill(&mut self.state, &ctx.host, &ctx.params),
            // step 04c: PTY 송신 핸들러. surface.send IPC를 통해 자식 terminal에
            // text를 보낸다.
            "claude.broadcast" => handle_broadcast(&self.state, &ctx.host, &ctx.params),
            "claude.tell" => handle_tell(&ctx.host, &ctx.params),
            // step 04d.1: 새 workspace에 claude 띄우기.
            "claude.launch" => handle_launch(&self.scanner, &ctx.host, &ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_surface_lifecycle(&mut self, ctx: SurfaceLifecycleCtx) {
        // 호스트가 일반 terminal surface가 닫혔다고 알려준다. 그 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. error scan에서도 함께 제외한다.
        let sid = ctx.surface_id;
        let parent_was_child = self.state.parent_of_child(sid).is_some();
        if parent_was_child {
            self.state.unregister_child(sid);
            self.state.save();
            if let Ok(mut s) = self.scanner.lock() {
                s.disable(sid);
            }
            return;
        }
        if self.state.is_known_parent(sid) {
            self.state.mark_parent_closed(sid);
            self.state.save();
        }
    }

    fn on_start(&mut self, host: HostHandle) {
        // worker dispatch가 시작되기 직전에 1회 호출. PTY error scan을 위한
        // background polling thread를 띄운다. 호스트가 메모리 스캔하던 패턴을
        // 1:1로 옮겼고 (`error_scan.rs::CLAUDE_ERROR_PATTERN`), polling 간격은
        // 800ms로 호스트 tick에 근접하게 맞춘다.
        let scanner = self.scanner.clone();
        std::thread::Builder::new()
            .name("claude-error-scan".into())
            .spawn(move || error_scan_loop(scanner, host))
            .expect("spawn claude-error-scan thread");
    }
}

// ─── step 04a 핸들러들 ───────────────────────────────────────────────────────
//
// 호스트 src/ipc/handler/claude.rs의 응답 JSON과 byte-for-byte 동일해야 cutover
// 후 CLI 출력 회귀가 없다. param 키 이름 / 응답 필드 / 누락된 surface_id의 에러
// 분기까지 1:1 보존한다.

fn require_surface_id(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'surface_id' parameter"))
}

fn handle_set_idle_state(state: &mut ClaudeState, params: &Value) -> Result<Value, IpcMethodError> {
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

fn handle_set_needs_input(
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
        .ok_or_else(|| {
            IpcMethodError::invalid_params("Missing 'needs_input' parameter (bool)")
        })?;
    state.set_needs_input(surface_id, needs_input);
    state.save();
    Ok(json!({ "ok": true }))
}

/// 호스트 `handle_claude_children` 1:1 이주. 자식 목록을 ClaudeState에서 읽고,
/// 각 자식의 PTY 전경 프로세스는 `surface.foreground_process` IPC로 조회한다.
/// IPC 실패는 무시 (host가 terminal을 못 찾으면 필드를 안 넣고 응답하던 동작과
/// 동일하게 None이 들어가 그 키들은 생략됨).
fn handle_children(
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
        if let Ok(resp) = host.call(
            "surface.foreground_process",
            json!({ "surface_id": sid }),
        ) {
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
fn children_base_entries(state: &ClaudeState, parent_surface_id: u32) -> Vec<Value> {
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
fn handle_wait(
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
enum WaitDecision {
    /// child가 ClaudeState에 없다 → 즉시 "exited".
    Exited,
    /// child가 state에 있다 → 호스트 트리에 surface가 살아있는지 확인 필요.
    /// 살아있으면 `state.state_of(child_surface_id)`, 죽었으면 "exited".
    CheckExistence(u32),
}

/// state만으로 결정 가능한 wait 분기. host IPC 없이 단위 테스트 가능.
fn wait_decide(state: &ClaudeState, parent_surface_id: u32, child_index: u32) -> WaitDecision {
    match state.find_child(parent_surface_id, child_index) {
        Some(c) => WaitDecision::CheckExistence(c.child_surface_id),
        None => WaitDecision::Exited,
    }
}

/// 호스트 `handle_claude_kill` 1:1 이주.
/// 1. ClaudeState에서 (parent_surface_id, child_index) → child_surface_id 해석
/// 2. `surface.locate` IPC로 pane_id 조회 (호스트의 `find_pane_for_surface`)
/// 3. `pane.close` IPC로 pane 제거 (호스트의 `close_pane_by_id` + 부수 효과)
/// 4. 성공 시 plugin state 정리 (unregister_child + mark_parent_closed)
fn handle_kill(
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
        .call(
            "surface.locate",
            json!({ "surface_id": child_surface_id }),
        )
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
fn kill_finalize(state: &mut ClaudeState, child_surface_id: u32) {
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
fn handle_broadcast(
    state: &ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params)?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing 'text' parameter"))?;
    let role_filter = params.get("role").and_then(|v| v.as_str()).map(String::from);

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
fn broadcast_targets(
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
fn handle_tell(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
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
fn handle_launch(
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

    let ws_resp = host
        .call(
            "workspace.create",
            json!({
                "type": "terminal",
                "name": workspace_name,
            }),
        )
        .map_err(IpcMethodError::from)?;

    let workspace_id = ws_resp
        .get("id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("workspace.create returned no 'id'"))?;
    let surface_id = ws_resp.get("surface_id").and_then(|v| v.as_u64()).map(|v| v as u32);

    if let Some(sid) = surface_id {
        if let Some(dir) = directory.as_deref() {
            let normalized = dir.replace('\\', "/");
            let escaped = shell_escape::escape(normalized.into());
            let _ = host.call(
                "surface.send",
                json!({ "surface_id": sid, "text": format!("cd {escaped}\r") }),
            );
        }

        let cmd = build_launch_command(task.as_deref());
        let _ = host.call(
            "surface.send",
            json!({ "surface_id": sid, "text": format!("{cmd}\r") }),
        );

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
fn build_launch_command(task: Option<&str>) -> String {
    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {escaped}"));
    }
    cmd
}

/// 호스트 코드의 PTY 시퀀스 생성 로직을 1:1 옮긴 순수 함수.
/// - 라인 사이: `\` + `\r` (Claude Code에서 newline 삽입)
/// - 마지막 라인이 `\`로 끝나면 ` ` 한 칸을 덧붙여 final `\r`이 submit으로 해석되게
/// - 끝에 `\r` 추가 = submit
fn build_tell_pty_text(message: &str) -> String {
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

fn handle_parent(state: &ClaudeState, params: &Value) -> Result<Value, IpcMethodError> {
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

#[cfg(test)]
mod handler_tests {
    use super::*;
    use state::{ChildEntry, ClaudeState};

    fn entry(child_surface_id: u32, index: u32) -> ChildEntry {
        ChildEntry {
            child_surface_id,
            index,
            cwd: None,
            role: None,
            nickname: None,
        }
    }

    #[test]
    fn set_idle_state_true_sets_idle() {
        let mut state = ClaudeState::default();
        let res = handle_set_idle_state(
            &mut state,
            &json!({ "surface_id": 5, "idle": true }),
        )
        .unwrap();
        assert_eq!(res, json!({ "ok": true }));
        assert_eq!(state.state_of(5), "idle");
    }

    #[test]
    fn set_idle_state_false_clears_idle_and_needs_input() {
        let mut state = ClaudeState::default();
        state.set_idle(5, true);
        state.set_needs_input(5, true);
        let _ = handle_set_idle_state(
            &mut state,
            &json!({ "surface_id": 5, "idle": false }),
        )
        .unwrap();
        assert_eq!(state.state_of(5), "active");
    }

    #[test]
    fn set_idle_state_missing_surface_id_returns_error() {
        let mut state = ClaudeState::default();
        let err = handle_set_idle_state(&mut state, &json!({ "idle": true })).unwrap_err();
        // 호스트는 No focused surface (-32000) 반환 — 호환 보존.
        assert_eq!(err.code, -32000);
    }

    #[test]
    fn set_idle_state_missing_idle_param_returns_invalid_params() {
        let mut state = ClaudeState::default();
        let err =
            handle_set_idle_state(&mut state, &json!({ "surface_id": 5 })).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn set_needs_input_true() {
        let mut state = ClaudeState::default();
        let res = handle_set_needs_input(
            &mut state,
            &json!({ "surface_id": 7, "needs_input": true }),
        )
        .unwrap();
        assert_eq!(res, json!({ "ok": true }));
        assert_eq!(state.state_of(7), "needs_input");
    }

    #[test]
    fn parent_returns_active_when_known() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 1));
        let res = handle_parent(&state, &json!({ "surface_id": 100 })).unwrap();
        assert_eq!(
            res,
            json!({ "parent_surface_id": 10, "status": "active" })
        );
    }

    #[test]
    fn parent_returns_closed_when_marked() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 1));
        state.mark_parent_closed(10);
        let res = handle_parent(&state, &json!({ "surface_id": 100 })).unwrap();
        assert_eq!(
            res,
            json!({ "parent_surface_id": 10, "status": "closed" })
        );
    }

    #[test]
    fn parent_returns_none_when_not_registered() {
        let state = ClaudeState::default();
        let res = handle_parent(&state, &json!({ "surface_id": 999 })).unwrap();
        assert_eq!(
            res,
            json!({ "parent_surface_id": null, "status": "none" })
        );
    }

    #[test]
    fn parent_missing_surface_id_is_invalid_params() {
        let state = ClaudeState::default();
        let err = handle_parent(&state, &json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    // ─── step 04b: children/wait/kill helper tests ──────────────────────────

    #[test]
    fn children_base_entries_empty_when_no_children() {
        let state = ClaudeState::default();
        assert!(children_base_entries(&state, 10).is_empty());
    }

    #[test]
    fn children_base_entries_includes_state_and_metadata() {
        let mut state = ClaudeState::default();
        state.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: 1,
                cwd: Some("/tmp".into()),
                role: Some("worker".into()),
                nickname: Some("alpha".into()),
            },
        );
        state.set_needs_input(100, true);
        let entries = children_base_entries(&state, 10);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e["child_surface_id"], 100);
        assert_eq!(e["index"], 1);
        assert_eq!(e["cwd"], "/tmp");
        assert_eq!(e["role"], "worker");
        assert_eq!(e["nickname"], "alpha");
        assert_eq!(e["state"], "needs_input");
        // foreground_process/foreground_pid는 IPC enrichment 단계 — 본 layer에서는
        // 키 자체가 존재하지 않아야 한다.
        assert!(e.get("foreground_process").is_none());
        assert!(e.get("foreground_pid").is_none());
    }

    #[test]
    fn wait_decide_returns_exited_when_child_unknown() {
        let state = ClaudeState::default();
        assert_eq!(wait_decide(&state, 10, 1), WaitDecision::Exited);
    }

    #[test]
    fn wait_decide_returns_check_existence_when_child_in_state() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 2));
        assert_eq!(
            wait_decide(&state, 10, 2),
            WaitDecision::CheckExistence(100)
        );
    }

    #[test]
    fn kill_finalize_removes_child_and_persists_only_when_needed() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 1));
        state.register_child(10, entry(101, 2));
        assert_eq!(state.list_children(10).len(), 2);
        kill_finalize(&mut state, 100);
        assert_eq!(state.list_children(10).len(), 1);
        assert_eq!(state.list_children(10)[0].index, 2);
        // unregister된 자식의 idle/needs_input 데이터도 함께 사라져야 한다.
        assert_eq!(state.parent_of_child(100), None);
    }

    // ─── step 04c: broadcast/tell helper tests ──────────────────────────────

    #[test]
    fn broadcast_targets_includes_all_children_without_filter() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 1));
        state.register_child(10, entry(101, 2));
        let ids = broadcast_targets(&state, 10, None);
        assert_eq!(ids, vec![100, 101]);
    }

    #[test]
    fn broadcast_targets_filters_by_role() {
        let mut state = ClaudeState::default();
        state.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: 1,
                cwd: None,
                role: Some("planner".into()),
                nickname: None,
            },
        );
        state.register_child(
            10,
            ChildEntry {
                child_surface_id: 101,
                index: 2,
                cwd: None,
                role: Some("worker".into()),
                nickname: None,
            },
        );
        state.register_child(
            10,
            ChildEntry {
                child_surface_id: 102,
                index: 3,
                cwd: None,
                role: Some("worker".into()),
                nickname: None,
            },
        );
        let ids = broadcast_targets(&state, 10, Some("worker"));
        assert_eq!(ids, vec![101, 102]);
    }

    #[test]
    fn broadcast_targets_empty_when_unknown_parent() {
        let state = ClaudeState::default();
        assert!(broadcast_targets(&state, 999, None).is_empty());
    }

    #[test]
    fn build_tell_pty_text_single_line_ends_with_cr() {
        assert_eq!(build_tell_pty_text("hello"), "hello\r");
    }

    #[test]
    fn build_tell_pty_text_multi_line_uses_backslash_cr() {
        // "a\nb" → "a\<CR>b<CR>"
        assert_eq!(build_tell_pty_text("a\nb"), "a\\\rb\r");
    }

    #[test]
    fn build_tell_pty_text_trailing_backslash_gets_space() {
        // 마지막 라인이 `\`로 끝나면 ` ` 삽입 후 `\r`.
        // "foo\\" → "foo\\ \r"
        assert_eq!(build_tell_pty_text("foo\\"), "foo\\ \r");
    }

    #[test]
    fn build_tell_pty_text_three_lines() {
        // "x\ny\nz" → "x\<CR>y\<CR>z<CR>"
        assert_eq!(build_tell_pty_text("x\ny\nz"), "x\\\ry\\\rz\r");
    }

    #[test]
    fn build_tell_pty_text_empty_message() {
        // "" → "\r" (single empty line + submit)
        assert_eq!(build_tell_pty_text(""), "\r");
    }

    // ─── step 04d.1: launch helper tests ────────────────────────────────────

    #[test]
    fn build_launch_command_no_task() {
        assert_eq!(build_launch_command(None), "claude");
    }

    #[test]
    fn build_launch_command_with_simple_task() {
        // shell_escape는 안전한 문자열을 그대로 둔다.
        assert_eq!(build_launch_command(Some("fix")), "claude --task fix");
    }

    #[test]
    fn build_launch_command_with_spaces_gets_escaped() {
        // 공백이 있으면 quote가 붙는다 — shell_escape의 표준 동작.
        let out = build_launch_command(Some("fix the bug"));
        assert!(
            out.starts_with("claude --task "),
            "prefix wrong: {out}"
        );
        // 'fix the bug'으로 single-quote escape 되거나 다른 안전 escape.
        assert!(out.contains("fix the bug"), "task body missing: {out}");
        assert_ne!(out, "claude --task fix the bug", "must be escaped");
    }

    #[test]
    fn kill_finalize_handles_nested_parent_case() {
        // child가 또 다른 parent를 가진 경우 (nested claude). mark_parent_closed가
        // 그 자식을 parent로 보고 closed_parents에 넣어야 한다.
        let mut state = ClaudeState::default();
        // 100은 10의 자식이면서 200/201의 부모.
        state.register_child(10, entry(100, 1));
        state.register_child(100, entry(200, 1));
        state.register_child(100, entry(201, 2));
        kill_finalize(&mut state, 100);
        // 100 자체는 10의 자식 자리에서 사라진다.
        assert_eq!(state.list_children(10).len(), 0);
        // 그러나 100을 부모로 하는 자식들은 그대로이고, 100이 closed로 마킹된다.
        assert!(state.is_parent_closed(100));
        assert_eq!(state.list_children(100).len(), 2);
    }
}

fn error_scan_loop(scanner: Arc<Mutex<ErrorScanner>>, host: HostHandle) {
    loop {
        std::thread::sleep(ERROR_SCAN_INTERVAL);
        // lock을 짧게 잡고 snapshot만 떠서 IPC 호출 동안 다른 메서드(enable/disable)가
        // 끼어들 수 있게 한다. snapshot 후 surface가 disable되면 다음 tick에 자연
        // 반영.
        let surfaces = match scanner.lock() {
            Ok(s) => s.enabled_snapshot(),
            Err(e) => {
                tracing::error!("claude scanner mutex poisoned: {e}");
                return;
            }
        };
        for sid in surfaces {
            // 각 IPC call은 최대 60초까지 block 가능하지만 정상 응답은 ms 단위.
            // 한 surface에서 timeout이 나도 나머지에 영향 없도록 그냥 진행.
            if let Ok(mut s) = scanner.lock() {
                let _ = s.scan_one(&host, sid);
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudePlugin::new())
}
