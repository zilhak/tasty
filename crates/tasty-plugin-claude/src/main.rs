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
    EventDispatchCtx, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx,
    SurfaceEventCtx, SurfaceResult,
};

use error_scan::ErrorScanner;
use state::{ChildEntry, ClaudeState};

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
            // step 04d.2: 자식 surface의 PTY를 갈아끼우고 claude 재시작.
            "claude.respawn" => handle_respawn(&mut self.state, &ctx.host, &ctx.params),
            // step 04d.3: parent surface가 사는 workspace 내 spawn pane을 자동
            // 관리(필요 시 생성)하고, 2x2 grid에 따라 새 자식 surface를 배치 +
            // claude 실행.
            "claude.spawn" => handle_spawn(&mut self.state, &ctx.host, &ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        // Event Bus 1.0: `surface.closed` 구독 시 호출. 닫힌 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. error scan에서도 함께 제외한다.
        if ctx.envelope.key != "surface.closed" {
            return;
        }
        let sid = match ctx.envelope.payload.get("surface_id").and_then(|v| v.as_u64()) {
            Some(v) => v as u32,
            None => return,
        };
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

    fn on_start(&mut self, host: HostHandle, bus: tasty_plugin_sdk::BusHandle) {
        // worker dispatch가 시작되기 직전에 1회 호출.
        // - `surface.closed` 이벤트 구독 (Event Bus 1.0). 옛 surface_observer
        //   매니페스트 필드의 대체 경로.
        // - PTY error scan을 위한 background polling thread spawn. 호스트가
        //   메모리 스캔하던 패턴을 1:1로 옮겼고 (`error_scan.rs::CLAUDE_ERROR_PATTERN`),
        //   polling 간격은 800ms로 호스트 tick에 근접하게 맞춘다.
        if let Err(e) = bus.subscribe("surface.closed") {
            tracing::warn!("subscribe surface.closed failed: {e}");
        }
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
            if let Err(e) = host.call(
                "surface.send",
                json!({ "surface_id": sid, "text": format!("cd {escaped}\r") }),
            ) {
                tracing::warn!("surface.send (cd) failed: {e}");
            }
        }

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
fn build_launch_command(task: Option<&str>) -> String {
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
fn handle_respawn(
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
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from);
    let role = params.get("role").and_then(|v| v.as_str()).map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let child_surface_id = state
        .find_child(parent_surface_id, child_index)
        .map(|c| c.child_surface_id)
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&format!(
                "Child index {} not found for parent {}",
                child_index, parent_surface_id
            ))
        })?;

    host.call(
        "surface.respawn_terminal",
        json!({ "surface_id": child_surface_id }),
    )
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

    start_claude_in_surface(host, child_surface_id, cwd.as_deref(), prompt.as_deref());

    Ok(json!({
        "child_surface_id": child_surface_id,
        "child_index": child_index,
        "parent_surface_id": parent_surface_id,
    }))
}

/// host code line 591-602의 metadata 부분 갱신 로직. None은 기존 값을 보존.
/// 반환: 한 필드라도 갱신됐는지 여부 (false면 save 호출 생략 가능).
fn update_child_metadata(
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
fn start_claude_in_surface(
    host: &HostHandle,
    surface_id: u32,
    cwd: Option<&str>,
    prompt: Option<&str>,
) {
    if let Some(dir) = cwd {
        let normalized = dir.replace('\\', "/");
        let escaped = shell_escape::escape(normalized.into());
        if let Err(e) = host.call(
            "surface.send",
            json!({ "surface_id": surface_id, "text": format!("cd {escaped}\r") }),
        ) {
            tracing::warn!("surface.send (cd) failed: {e}");
        }
    }

    let agent_id = format!("claude_s{surface_id}");
    let session_token = issue_session_token(host, &agent_id);
    let agent_prefix = match session_token {
        Some(tok) => format!("TASTY_AGENT_ID={agent_id} TASTY_SESSION_TOKEN={tok} "),
        None => format!("TASTY_AGENT_ID={agent_id} "),
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
fn issue_session_token(host: &HostHandle, agent_id: &str) -> Option<String> {
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
fn handle_spawn(
    state: &mut ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = caller_surface_id(params).ok_or_else(|| {
        IpcMethodError::invalid_params(
            "Cannot determine parent surface. Set TASTY_SURFACE_ID.",
        )
    })?;
    let workspace_param = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required '--workspace' parameter"))?
        .to_string();
    let cwd = params.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let role = params.get("role").and_then(|v| v.as_str()).map(String::from);
    let nickname = params
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let ws_id = resolve_workspace_id(host, &workspace_param)?.ok_or_else(|| {
        IpcMethodError::invalid_params(&format!("Workspace '{}' not found", workspace_param))
    })?;

    let spawn_pane_id = resolve_or_create_spawn_pane(state, host, parent_surface_id, ws_id)?;
    let child_surface_id = find_and_spawn_in_pane(host, spawn_pane_id)?;

    let child_index = state.next_child_index(parent_surface_id);
    state.register_child(
        parent_surface_id,
        ChildEntry {
            child_surface_id,
            index: child_index,
            cwd: cwd.clone(),
            role: role.clone(),
            nickname: nickname.clone(),
        },
    );
    state.save();

    start_claude_in_surface(host, child_surface_id, cwd.as_deref(), prompt.as_deref());

    Ok(json!({
        "child_surface_id": child_surface_id,
        "child_index": child_index,
        "parent_surface_id": parent_surface_id,
        "spawn_pane_id": spawn_pane_id,
        "workspace_id": ws_id,
    }))
}

/// 호스트 `caller_surface_id` 1:1. plugin IPC ctx.params에 같은 키가 들어온다.
fn caller_surface_id(params: &Value) -> Option<u32> {
    params
        .get("caller_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
}

/// 호스트 `resolve_workspace` 1:1. target이 숫자면 id, 아니면 name으로 매칭.
fn resolve_workspace_id(host: &HostHandle, target: &str) -> Result<Option<u32>, IpcMethodError> {
    let ws_list = host
        .call("workspace.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let arr = ws_list.as_array().ok_or_else(|| {
        IpcMethodError::new("workspace.list returned non-array")
    })?;
    if let Ok(target_id) = target.parse::<u32>() {
        for w in arr {
            if w.get("id").and_then(|v| v.as_u64()) == Some(target_id as u64) {
                return Ok(Some(target_id));
            }
        }
    }
    for w in arr {
        if w.get("name").and_then(|v| v.as_str()) == Some(target) {
            if let Some(id) = w.get("id").and_then(|v| v.as_u64()) {
                return Ok(Some(id as u32));
            }
        }
    }
    Ok(None)
}

/// state.spawn_panes의 캐시된 pane_id가 여전히 유효한지 검증하고, 아니면 새
/// spawn pane을 만든다. 반환은 유효한 spawn_pane_id.
fn resolve_or_create_spawn_pane(
    state: &mut ClaudeState,
    host: &HostHandle,
    parent_surface_id: u32,
    ws_id: u32,
) -> Result<u32, IpcMethodError> {
    let cached = state.spawn_pane_for(parent_surface_id, ws_id);
    let panes = host
        .call("pane.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let panes_arr = panes
        .as_array()
        .ok_or_else(|| IpcMethodError::new("pane.list returned non-array"))?;

    // 캐시된 pane이 같은 workspace에 여전히 존재하면 그대로 사용.
    if let Some(pid) = cached {
        let still_valid = panes_arr.iter().any(|p| {
            p.get("id").and_then(|v| v.as_u64()) == Some(pid as u64)
                && p.get("workspace_id").and_then(|v| v.as_u64()) == Some(ws_id as u64)
        });
        if still_valid {
            return Ok(pid);
        }
        // stale 매핑 정리.
        state.clear_spawn_pane(parent_surface_id, ws_id);
    }

    // 새 spawn pane 생성: workspace 내 임의의 pane을 vertical로 split.
    let any_pane_in_ws = panes_arr
        .iter()
        .find(|p| p.get("workspace_id").and_then(|v| v.as_u64()) == Some(ws_id as u64))
        .and_then(|p| p.get("id").and_then(|v| v.as_u64()).map(|v| v as u32))
        .ok_or_else(|| IpcMethodError::new(format!("No panes in workspace {ws_id}")))?;

    let split_resp = host
        .call(
            "split",
            json!({
                "level": "pane",
                "target_pane": any_pane_in_ws,
                "direction": "vertical",
                "type": "terminal",
            }),
        )
        .map_err(IpcMethodError::from)?;
    let new_pane_id = split_resp
        .get("new_pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::new("split returned no 'new_pane_id'"))?;

    state.set_spawn_pane(parent_surface_id, ws_id, new_pane_id);
    Ok(new_pane_id)
}

/// 호스트 `find_and_spawn_in_pane` 1:1. spawn pane 안에서 첫 빈 slot(< 4개의
/// surface)을 찾아 surface-level split으로 새 surface를 만들고 ID 반환. 모든
/// 탭이 가득 차면 새 탭을 만든다.
fn find_and_spawn_in_pane(host: &HostHandle, spawn_pane_id: u32) -> Result<u32, IpcMethodError> {
    let tabs = collect_pane_tab_surfaces(host, spawn_pane_id)?;

    // 첫 < 4 인 tab에서 split target을 결정.
    if let Some((_, surfaces)) = tabs.iter().find(|(_, sids)| sids.len() < 4) {
        let (target_sid, direction) = pick_split_target(surfaces.len(), surfaces);
        let split_resp = host
            .call(
                "split",
                json!({
                    "level": "surface",
                    "target_surface": target_sid,
                    "direction": direction,
                    "type": "terminal",
                }),
            )
            .map_err(IpcMethodError::from)?;
        let new_sid = split_resp
            .get("new_surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| IpcMethodError::new("split returned no 'new_surface_id'"))?;
        return Ok(new_sid);
    }

    // 모든 탭 가득 — 새 탭 생성. tab.create는 surface_id를 반환하지 않으므로
    // 생성 직후의 surface.list로 새 탭(index = tabs.len())의 유일한 surface를
    // 찾는다.
    let resp = host
        .call(
            "tab.create",
            json!({ "pane_id": spawn_pane_id, "type": "terminal" }),
        )
        .map_err(IpcMethodError::from)?;
    let new_tab_count = resp
        .get("tab_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .ok_or_else(|| IpcMethodError::new("tab.create returned no 'tab_count'"))?;
    let new_tab_index = new_tab_count.saturating_sub(1);

    let surfaces = host
        .call("surface.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let arr = surfaces
        .as_array()
        .ok_or_else(|| IpcMethodError::new("surface.list returned non-array"))?;
    let new_sid = arr
        .iter()
        .find(|s| {
            s.get("pane_id").and_then(|v| v.as_u64()) == Some(spawn_pane_id as u64)
                && s.get("tab_index").and_then(|v| v.as_u64()) == Some(new_tab_index as u64)
        })
        .and_then(|s| s.get("id").and_then(|v| v.as_u64()).map(|v| v as u32))
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "tab.create succeeded but no surface found in pane={spawn_pane_id} tab_index={new_tab_index}"
            ))
        })?;
    Ok(new_sid)
}

/// pane 내부의 tab별 surface_id 목록을 tab_index 순서로 수집. surface.list가
/// 이미 collect_tab_surfaces에서 first-then-second 표시 순서를 보존하므로
/// 같은 순서로 자연히 정렬된다.
fn collect_pane_tab_surfaces(
    host: &HostHandle,
    pane_id: u32,
) -> Result<Vec<(usize, Vec<u32>)>, IpcMethodError> {
    let surfaces = host
        .call("surface.list", json!({}))
        .map_err(IpcMethodError::from)?;
    let arr = surfaces
        .as_array()
        .ok_or_else(|| IpcMethodError::new("surface.list returned non-array"))?;
    let mut by_tab: std::collections::BTreeMap<usize, Vec<u32>> = std::collections::BTreeMap::new();
    for s in arr {
        if s.get("pane_id").and_then(|v| v.as_u64()) != Some(pane_id as u64) {
            continue;
        }
        let tab_idx = s.get("tab_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let sid = s.get("id").and_then(|v| v.as_u64()).map(|v| v as u32);
        if let Some(sid) = sid {
            by_tab.entry(tab_idx).or_default().push(sid);
        }
    }
    Ok(by_tab.into_iter().collect())
}

/// 호스트 `pick_split_target` 1:1.
/// - 0/1 surface: 그 surface를 vertical로 split (left|right 생성)
/// - 2: surface_ids[0]을 horizontal로 split (left-top|left-bottom + right)
/// - 3: surface_ids[2]을 horizontal로 split (right-top|right-bottom)
fn pick_split_target(count: usize, surface_ids: &[u32]) -> (u32, &'static str) {
    match count {
        0 | 1 => (surface_ids.first().copied().unwrap_or(0), "vertical"),
        2 => (surface_ids[0], "horizontal"),
        3 => (surface_ids[2], "horizontal"),
        _ => (surface_ids.last().copied().unwrap_or(0), "vertical"),
    }
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
        handle_set_idle_state(
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

    // ─── step 04d.2: respawn helper tests ───────────────────────────────────

    #[test]
    fn update_child_metadata_noop_when_all_none() {
        let mut state = ClaudeState::default();
        state.register_child(10, entry(100, 1));
        let updated = update_child_metadata(&mut state, 10, 1, None, None, None);
        assert!(!updated, "should report no update when all fields are None");
    }

    #[test]
    fn update_child_metadata_overwrites_only_given_fields() {
        let mut state = ClaudeState::default();
        state.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: 1,
                cwd: Some("/old".into()),
                role: Some("old_role".into()),
                nickname: Some("old_nick".into()),
            },
        );
        let updated =
            update_child_metadata(&mut state, 10, 1, Some("/new"), None, Some("new_nick"));
        assert!(updated);
        let e = state.find_child(10, 1).unwrap();
        assert_eq!(e.cwd.as_deref(), Some("/new"));
        // role은 None이었으므로 보존되어야 한다.
        assert_eq!(e.role.as_deref(), Some("old_role"));
        assert_eq!(e.nickname.as_deref(), Some("new_nick"));
    }

    #[test]
    fn update_child_metadata_returns_false_when_child_missing() {
        let mut state = ClaudeState::default();
        // 자식 등록 없음. 그래도 cwd가 주어졌으므로 attempt는 발생 — 그러나
        // update_child가 child 없음으로 false 반환 → wrapper도 false.
        let updated = update_child_metadata(&mut state, 10, 1, Some("/x"), None, None);
        assert!(!updated);
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

    // ─── step 04d.3: spawn helper tests ─────────────────────────────────────

    #[test]
    fn caller_surface_id_reads_key_from_params() {
        assert_eq!(
            caller_surface_id(&json!({ "caller_surface_id": 42 })),
            Some(42)
        );
    }

    #[test]
    fn caller_surface_id_missing_returns_none() {
        assert_eq!(caller_surface_id(&json!({})), None);
    }

    #[test]
    fn caller_surface_id_wrong_type_returns_none() {
        assert_eq!(
            caller_surface_id(&json!({ "caller_surface_id": "42" })),
            None
        );
    }

    #[test]
    fn pick_split_target_zero_surfaces_uses_vertical() {
        // empty slice: fallback path uses 0 as target.
        let (sid, dir) = pick_split_target(0, &[]);
        assert_eq!(sid, 0);
        assert_eq!(dir, "vertical");
    }

    #[test]
    fn pick_split_target_one_surface_splits_vertical() {
        // 1 surface in tab → split vertically to create left|right (count becomes 2).
        let (sid, dir) = pick_split_target(1, &[10]);
        assert_eq!(sid, 10);
        assert_eq!(dir, "vertical");
    }

    #[test]
    fn pick_split_target_two_surfaces_splits_first_horizontal() {
        // 2 surfaces (left|right) → split left horizontally → 3 surfaces.
        let (sid, dir) = pick_split_target(2, &[10, 20]);
        assert_eq!(sid, 10);
        assert_eq!(dir, "horizontal");
    }

    #[test]
    fn pick_split_target_three_surfaces_splits_third_horizontal() {
        // 3 surfaces (left-top|left-bottom + right) → split right horizontally → 2x2.
        let (sid, dir) = pick_split_target(3, &[10, 20, 30]);
        assert_eq!(sid, 30);
        assert_eq!(dir, "horizontal");
    }

    #[test]
    fn spawn_pane_cache_round_trip_via_state() {
        // resolve_or_create_spawn_pane은 HostHandle을 필요로 해서 직접 테스트는
        // 어렵지만, state-level 캐시 동작은 핵심이므로 검증한다.
        let mut state = ClaudeState::default();
        assert_eq!(state.spawn_pane_for(10, 5), None);
        state.set_spawn_pane(10, 5, 77);
        assert_eq!(state.spawn_pane_for(10, 5), Some(77));
        // 다른 (parent, workspace) 조합은 영향 없음.
        assert_eq!(state.spawn_pane_for(11, 5), None);
        assert_eq!(state.spawn_pane_for(10, 6), None);
        state.clear_spawn_pane(10, 5);
        assert_eq!(state.spawn_pane_for(10, 5), None);
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
                // 반환값(매치된 snippet)은 단위 테스트용. polling 루프에서는 무시.
                s.scan_one(&host, sid);
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
