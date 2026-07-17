//! `handle_ipc_method` 내부에서 각 codex.* 메서드를 처리한다.
//!
//! 자식 terminal 관리(spawn/tell/children/parent/kill/respawn/broadcast)는
//! 호스트가 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 **위임**한다.
//! 이 plugin 은 더 이상 자체 child registry 를 보유하지 않는다(호스트 registry 가
//! 단일 SoT). 여기 남는 것은 codex **특화**뿐:
//! - `make_codex_command` — codex 바이너리 기동 명령 빌더(`--dangerously-bypass-hook-trust`
//!   포함 — hook 이 항상 fire 되게 한다).
//! - install/uninstall/hook — `~/.codex/config.toml` 조작 + trust 판정.
//! - hook 이 산출한 idle/active 신호를 `terminal.set_state` 로 호스트 registry 에 주입하고,
//!   `stop` 이벤트는 `surface.fire_hook`으로 `codex-idle`도 함께 쏜다.
//! - `handle_spawn`/`handle_tell` 이 완료 시(`codex-idle`/`process-exit`) caller 에게
//!   1 회성 알림을 보내는 hook 을 등록한다(`register_notify_hooks`).
//!
//! 모든 호스트 호출은 `host.call(...)`을 통해 동기로 이루어진다.

use serde_json::{Map, Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

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

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// 요청 params 에서 지정한 키들을 존재할 때만 그대로 새 Map 에 복사한다. 자식
/// 관리 명령을 호스트 `terminal.*` 로 위임할 때 CLI 인자를 pass-through 하는 용도.
fn forward(params: &Value, keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = params.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

/// codex 명령을 PTY로 보낼 문자열을 만든다. prompt가 있으면 shell quote.
///
/// `TASTY_SURFACE_ID={surface_id}` inline env prefix를 항상 박는다. 이게 없으면
/// codex 프로세스 env에 `TASTY_SURFACE_ID`가 비어, `~/.codex/config.toml`의 hook
/// 명령 (`tasty codex hook X --surface $TASTY_SURFACE_ID`)이 surface ID 없이
/// 실행되어 `handle_hook`이 invalid_params로 거부 → idle/needs_input 상태가 영원히
/// 갱신되지 않는다. claude plugin의 `start_claude_in_surface`와 동일한 패턴.
///
/// `--dangerously-bypass-hook-trust` 는 사용자가 `/hooks` 로 수동 승인하기 전에도
/// tasty 가 install 한 hook 이 항상 fire 되게 한다. tasty 는 자기 hook을 스스로
/// 심으므로(hook source 를 스스로 vet함) 이 플래그의 정당한 사용 대상이다 —
/// 이게 없으면 codex 가 hook 을 fire 하지 않아 `codex-idle` 알림이 영원히 오지
/// 않는다.
fn make_codex_command(surface_id: u32, prompt: Option<&str>) -> String {
    let prefix = format!("TASTY_SURFACE_ID={surface_id} ");
    match prompt {
        Some(p) if !p.is_empty() => {
            let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
            format!("{prefix}codex --dangerously-bypass-hook-trust \"{escaped}\"\r")
        }
        _ => format!("{prefix}codex --dangerously-bypass-hook-trust\r"),
    }
}

pub fn handle_launch(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
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

pub fn handle_parent(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    // 호스트 registry 가 parent 매핑의 SoT — 그대로 위임.
    let surface = require_u32(&params, "surface")?;
    host_call(host, "terminal.parent", json!({ "surface": surface }))
}

pub fn handle_tell(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let surface_id = require_u32(&params, "surface")?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'message'"))?;
    // 개행/제출 규칙(단일라인 평문 / 멀티라인 bracketed paste + 별도 `\r`)은 호스트
    // `terminal.tell` 이 동일하게 처리한다 → 본문 포맷을 재구현하지 않고 위임.
    let resp = host_call(
        host,
        "terminal.tell",
        json!({ "surface": surface_id, "text": message }),
    )?;

    // caller_surface 는 dynamic CLI 가 `TASTY_SURFACE_ID` 로 자동 채운다(명시
    // --caller-surface 도 허용). 없으면(예: 호스트가 직접 IPC 호출) 완료 알림을
    // 등록하지 않는다 — 누구에게 알릴지 모르므로.
    if let Ok(caller) = require_u32(&params, "caller_surface") {
        register_notify_hooks(host, surface_id, caller, "tell");
    }

    Ok(resp)
}

pub fn handle_spawn(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let parent_surface = require_u32(&params, "surface")?;
    let prompt = optional_str(&params, "prompt");

    // 1) 호스트 registry 에 자식 등록 + soft 점유 + tab 생성 (command 미전송).
    //    workspace 는 required — 없으면 호스트가 invalid_params 로 거부한다.
    let mut sp = forward(&params, &["workspace", "pane", "cwd", "role", "nickname"]);
    sp.insert("parent".into(), json!(parent_surface));
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "terminal.spawn response missing 'child_surface_id': {resp}"
            ))
        })?;

    // 2) codex 특화 기동 명령을 그 surface 에 전송(surface_id inline env 필요).
    let cmd = make_codex_command(child_sid, prompt.as_deref());
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    // 3) 완료 시(codex-idle/process-exit) parent 에게 1 회성 알림 등록.
    register_notify_hooks(host, child_sid, parent_surface, "spawn");

    // 4) child 개수 임계치 경고(soft) — spawn 자체를 막지 않는다.
    let mut out = resp;
    if let Some(warning) = compute_spawn_warning(host, parent_surface) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("warning".into(), json!(warning));
        }
    }

    Ok(out)
}

/// 호스트 IPC 를 동기 호출하는 최소 표면. `HostHandle` 로 실동작하고, 테스트에서는
/// in-memory mock 으로 대체해 형제 hook 등록/발화/정리 사이클을 재현·검증한다.
pub(crate) trait HostCall {
    fn call(&self, method: &str, params: Value) -> Result<Value, tasty_plugin_sdk::PluginError>;
}

impl HostCall for HostHandle {
    fn call(&self, method: &str, params: Value) -> Result<Value, tasty_plugin_sdk::PluginError> {
        HostHandle::call(self, method, params)
    }
}

/// 완료 알림 hook 의 command 문자열 — 등록 시점과 fire 후 정리 시점이 **정확히 같은
/// 값**을 만들어야 command 일치 정리가 성립한다. 형제(codex-idle/process-exit)는 모두
/// 이 동일 문자열을 command 로 갖는다.
fn notify_caller_command(caller_surface: u32, target_surface: u32, kind: &str) -> String {
    format!(
        "tasty codex notify-caller --caller {caller_surface} --target {target_surface} --kind {kind}"
    )
}

/// caller 에게 보여줄 완료 알림 문구 — "그 child 가 맡은 작업이 끝났다"를 앞세운다.
/// 과거 `"{kind} 완료: surface {target}"` 형태는 spawn/tell 자체가(호출이) 완료됐다는
/// 뜻으로 오독되기 쉬워, conductor 가 실제 작업 완료 알림을 "spawn 접수 확인" 정도로
/// 여기고 계속 무시하는 사고로 이어졌다. `kind`는 호출 방식(spawn/tell)일 뿐 완료의
/// 주어가 아니므로 괄호로 분리한다(tasty-plugin-claude 의 `notify_done_message`와 동형).
fn notify_caller_message(kind: &str, target: u32) -> String {
    format!("surface {target} 작업 완료 (호출 방식: {kind})")
}

/// `hook.list` 응답 배열에서 정리 대상 형제 hook 의 id 들을 고른다 — command 문자열이
/// `expected_command` 와 정확히 일치하는 hook 만. 상태를 공유하지 않는(clobber 불가)
/// 순수 선택이라 concurrent 등록에도 그룹 격리가 성립한다: 같은 target surface 에
/// 서로 다른 command(예: `--kind spawn` vs `--kind tell`)로 등록된 두 그룹은 서로의
/// 정리 대상에 포함되지 않는다(옛 단일 meta 슬롯 방식의 clobber-좀비를 제거).
fn siblings_to_unset(hooks: &[Value], expected_command: &str) -> Vec<u64> {
    hooks
        .iter()
        .filter(|h| h.get("command").and_then(|v| v.as_str()) == Some(expected_command))
        .filter_map(|h| h.get("id").and_then(|v| v.as_u64()))
        .collect()
}

/// 발화한 형제 하나가 자기 그룹(같은 command)의 남은 형제 once-hook 들을 정리한다.
/// `hook.list` 는 반드시 `surface_id` 로 필터해 다른 surface(=다른 child)의 hook 을
/// 건드리지 않는다. best-effort — 실패해도 알림 자체는 이미 전달됐다.
fn cleanup_sibling_hooks<H: HostCall>(host: &H, target_surface: u32, expected_command: &str) {
    if let Ok(resp) = host.call("hook.list", json!({ "surface_id": target_surface }))
        && let Some(hooks) = resp.as_array()
    {
        for hook_id in siblings_to_unset(hooks, expected_command) {
            // best-effort 정리 — 실패하면 좀비로 남을 수 있으나 알림 자체는 이미
            // 전달됐으므로 caller 관점 결과에는 영향 없음.
            let _ = host.call("hook.unset", json!({ "hook_id": hook_id }));
        }
    }
}

/// child(=target) 가 완료(codex-idle 또는 process-exit)되면 caller 에게 1 회성 알림을
/// 보내도록 hook 2개를 등록한다. 두 hook 의 command 는 완전히 동일한 `codex
/// notify-caller` 호출이며, fire 시점에 `hook.list` 를 command 문자열로 매칭해 자기
/// 그룹의 남은 형제를 정리한다 — 어느 이벤트가 먼저 fire 하는지에 무관하게 대칭적으로
/// 동작하고, 상태(단일 meta 슬롯)를 공유하지 않아 같은 surface 에 spawn/tell 이 겹쳐
/// 등록돼도 서로의 형제를 덮어써 좀비로 남기지 않는다. host 호출 실패는 경고만 하고
/// 넘어간다(soft — spawn/tell 성공을 막지 않음).
fn register_notify_hooks<H: HostCall>(
    host: &H,
    target_surface: u32,
    caller_surface: u32,
    kind: &str,
) {
    let cmd = notify_caller_command(caller_surface, target_surface, kind);
    for event in ["codex-idle", "process-exit"] {
        if let Err(e) = host.call(
            "hook.set",
            json!({ "surface_id": target_surface, "event": event, "command": cmd, "once": true }),
        ) {
            tracing::warn!("codex notify hook.set '{event}' failed: {e}");
        }
    }
}

/// `register_notify_hooks` 가 등록한 hook 이 fire 되면 실행되는 핸들러. caller
/// 에게 완료 알림을 보내고, 형제 once-hook(자신 포함)을 함께 정리한다. 자신은
/// once 시맨틱으로 이미 자동 제거된 뒤이므로 unset 이 no-op 이어도 무해하다 —
/// "누가 먼저 fire했는지" 판별이 전혀 필요 없다. 정리는 `hook.list`(surface 필터) +
/// command 문자열 일치로 하며, 상태(단일 meta 슬롯)를 공유하지 않아 같은 surface 에
/// spawn/tell 이 겹쳐 등록돼도 서로의 형제를 덮어써 좀비로 남기지 않는다.
pub fn handle_notify_caller<H: HostCall>(host: &H, params: Value) -> Result<Value, IpcMethodError> {
    let caller = require_u32(&params, "caller")?;
    let target = require_u32(&params, "target")?;
    let kind = optional_str(&params, "kind").unwrap_or_else(|| "tell".into());
    let message = notify_caller_message(&kind, target);

    // 완료 로그 파일에 append — conductor 가 Monitor tool 로 tail 하면 busy/idle 여부와
    // 무관하게 다음 턴에 전달된다. 완료 알림의 유일한 경로다(과거엔 terminal.tell 도
    // 함께 발사했으나, 자동 이벤트가 실제 사용자 발화처럼 대화 트랜스크립트에 섞여
    // 들어가는 부작용 때문에 제거함). best-effort — 실패해도 hook 정리에 영향 없음.
    if let Err(e) = tasty_utils::notify::append_notify_line(caller, &message) {
        tracing::warn!("codex notify-caller completion-log append failed: {e}");
    }

    // 자기 그룹(같은 command)의 남은 형제 정리 — surface 필터 + command 일치.
    let expected_command = notify_caller_command(caller, target, &kind);
    cleanup_sibling_hooks(host, target, &expected_command);

    // target 이 아직 살아있다면(이번 fire 가 process-exit 가 아니었다면) 형제 hook 을
    // 다시 등록해 다음 idle 전환에도 알림이 오도록 자기재무장한다 — "spawn/tell 당
    // 알림 1회" 가 아니라 "child 가 살아있는 동안 상태 전환마다 알림"으로 바뀐다.
    rearm_if_still_alive(host, caller, target, &kind);

    Ok(json!({}))
}

/// `target` 이 host 트리에 여전히 존재하면(=이번 fire 가 process-exit 가 아니었다면)
/// 형제 hook(codex-idle/process-exit)을 재등록한다. `surface.locate` 로 생존을
/// 판별하는 이유: process-exit 로 fire 된 경우 host 는 hook 발화 직후 동기로 그
/// surface 를 이미 닫으므로(`close_surface_by_id_no_snapshot`), 이 시점에 조회하면
/// 사라져 있다 — 반대로 codex-idle 은 surface 가 살아있는 상태에서만 발생하는
/// 이벤트라 재등록이 안전하다. 조회 실패(best-effort)는 "죽었다"로 간주해 재등록을
/// 건너뛴다 — 좀비 hook 을 쌓는 것보다 드물게 재무장을 놓치는 쪽이 안전하다.
fn rearm_if_still_alive<H: HostCall>(host: &H, caller: u32, target: u32, kind: &str) {
    let alive = host
        .call("surface.locate", json!({ "surface_id": target }))
        .ok()
        .and_then(|r| r.get("exists").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if alive {
        // register_notify_hooks 는 (host, target_surface, caller_surface, kind) 순서다
        // (claude 쪽과 인자 순서가 다르므로 주의).
        register_notify_hooks(host, target, caller, kind);
    }
}

const DEFAULT_SPAWN_CHILD_WARN_THRESHOLD: f64 = 6.0;

/// spawn 직후 parent 의 현재 child 목록/상태를 재조회해 임계치 초과 여부를 판단한다.
/// host 호출 실패는 경고 생략으로 처리한다(soft 경고이므로 spawn 성공을 막지 않음).
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

/// child 개수가 임계치를 넘으면 경고 문구를 만든다(순수 함수, 단위 테스트 대상).
/// idle child 가 있으면 그 index 목록과 `respawn` 권유 문구를 덧붙인다.
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

pub fn handle_children(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    host_call(
        host,
        "terminal.children",
        Value::Object(forward(&params, &["surface"])),
    )
}

pub fn handle_broadcast(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'text'"))?;
    let mut bp = forward(&params, &["surface", "role"]);
    bp.insert("text".into(), json!(text));
    host_call(host, "terminal.broadcast", Value::Object(bp))
}

pub fn handle_kill(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let child = require_u32(&params, "child")?;
    let mut kp = forward(&params, &["surface"]);
    kp.insert("child".into(), json!(child));
    host_call(host, "terminal.kill", Value::Object(kp))
}

pub fn handle_respawn(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let child = require_u32(&params, "child")?;
    let prompt = optional_str(&params, "prompt");

    // 1) 호스트 registry 위임: cwd 있으면 PTY 교체, 없으면 Ctrl-C. role/nickname/cwd
    //    갱신 + idle 초기화까지 호스트가 수행하고 child_surface_id 를 돌려준다.
    //    codex 기동은 여기서 하지 않으므로 command 는 넘기지 않는다.
    let mut rp = forward(&params, &["surface", "cwd", "role", "nickname"]);
    rp.insert("child".into(), json!(child));
    let resp = host_call(host, "terminal.respawn", Value::Object(rp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(format!(
                "terminal.respawn response missing 'child_surface_id': {resp}"
            ))
        })?;

    // 2) codex 특화 기동 명령 재전송.
    let cmd = make_codex_command(child_sid, prompt.as_deref());
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    Ok(resp)
}

/// Codex CLI hook event 가 fire 됐을 때 호출. install 이 박은 `Stop` /
/// `UserPromptSubmit` / `SessionStart` 만 정상 처리한다. idle/active 신호를
/// 호스트 registry(`terminal.set_state`)에 주입한다 — 자체 state 는 없다.
///
/// **반환값**: 빈 객체 `{}`. CLI 의 stdout 으로 흘러나가 codex 가 직접 파싱하므로
/// codex 의 wire schema 와 호환되어야 한다. 모든 필드가 optional 이므로 empty
/// object 는 "no decision, continue normally" 의미.
pub fn handle_hook(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'event'"))?;
    let surface_id = require_u32(&params, "surface")
        .map_err(|_| IpcMethodError::invalid_params("hook requires --surface to identify child"))?;
    let new_state = hook_event_to_state(event)?;
    // session-start 에 session id(stdin JSON `session_id` → CLI `--session`)가
    // 오면 reboot/복원용 세션 meta 를 기록한다. codex 에는 SessionEnd hook 이
    // 없어 unset 경로는 없다 — 다음 session-start 가 덮어쓴다. resume 기동도
    // source=resume 인 session-start 를 같은 session_id 로 다시 fire 한다(실측).
    if event == "session-start"
        && let Some(session) = params.get("session").and_then(|v| v.as_str())
        && !session.is_empty()
    {
        for (key, value) in [
            ("codex-session-id", session.to_string()),
            ("restore.command", format!("codex resume {session}")),
        ] {
            if let Err(e) = host.call(
                "surface.meta.set",
                json!({ "surface_id": surface_id, "key": key, "value": value }),
            ) {
                tracing::warn!("codex hook meta.set '{key}' failed: {e}");
            }
        }
    }
    host_call(
        host,
        "terminal.set_state",
        json!({ "surface": surface_id, "state": new_state }),
    )?;
    // stop → idle 은 완료 신호이기도 하므로 `codex-idle` surface hook 도 함께
    // 쏜다 — `register_notify_hooks` 로 등록된 1 회성 알림이 이걸 구독한다.
    if event == "stop"
        && let Err(e) = host.call(
            "surface.fire_hook",
            json!({ "surface_id": surface_id, "event": "codex-idle" }),
        )
    {
        tracing::warn!("codex hook fire_hook 'codex-idle' failed: {e}");
    }
    Ok(json!({}))
}

/// codex hook event → 호스트 registry state 매핑(순수 함수, 단위 테스트 가능).
fn hook_event_to_state(event: &str) -> Result<&'static str, IpcMethodError> {
    match event {
        "stop" => Ok("idle"),
        "prompt-submit" | "session-start" => Ok("active"),
        other => Err(IpcMethodError::invalid_params(&format!(
            "unknown hook event '{other}' (supported: stop, prompt-submit, session-start)"
        ))),
    }
}

pub fn handle_install() -> Result<Value, IpcMethodError> {
    let path = codex_config_toml_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| IpcMethodError::new(format!("mkdir failed: {e}")))?;
    }
    let existing = read_toml_or_default(&path);
    let merged = merge_install(existing);
    write_toml(&path, &merged)?;
    let trusted = codex_hooks_all_trusted();
    let mut resp = json!({
        "installed": true,
        "path": path.to_string_lossy(),
        "trust_status": if trusted { "trusted" } else { "needs_review" },
    });
    if !trusted {
        resp["note"] = Value::String(
            "Codex blocks newly-added hooks until trusted, but tasty starts every codex instance \
with `--dangerously-bypass-hook-trust` (spawn/launch/reboot), so hooks fire regardless of this \
status. Manual trust is only needed if you run `codex` yourself without that flag. To trust \
manually: run `codex` in any terminal, type `/hooks` + Enter, then for each of 3 hooks press \
Enter → t → Esc → Down. Trust persists per-machine."
                .into(),
        );
    }
    Ok(resp)
}

pub fn handle_uninstall() -> Result<Value, IpcMethodError> {
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
// 의 `trusted_hash` 메커니즘). install 자체는 멱등하게 entry 를 박지만, 승인
// 없이는 hook 이 fire 되지 않는다 — **단, `--dangerously-bypass-hook-trust`
// CLI 플래그(codex 공식 옵션)를 기동 명령에 박으면 이 승인 절차를 우회할 수
// 있다**(`make_codex_command`/`reboot::resume_command` 가 항상 이 플래그를
// 붙인다). tasty 는 자기 hook 을 스스로 심으므로(hook source 를 스스로 vet함)
// 이 플래그의 정당한 사용 대상이다. `codex_hooks_all_trusted*` 는 이제 wait
// 경로가 아니라 `handle_install` 의 안내 문구(수동 승인 여부 표시)에만 쓰인다.

use std::path::{Path, PathBuf};

const HOOK_MARKER: &str = "tasty codex hook";

/// (camel for `[hooks.<Camel>]` table key, kebab for `tasty codex hook <kebab>` CLI
/// subcommand, snake for `[hooks.state."<path>:<snake>:0:0"]` trust state key).
///
/// 3 컬럼이 다른 케이스를 쓰는 이유: codex 가 같은 event 를 표면별로 다른 표기로
/// 인코딩한다. config table 키는 Rust enum variant 그대로 CamelCase, hook 명령에
/// 넘기는 우리 자체 event 이름은 kebab, codex 가 trust state 를 영속화할 때 쓰는
/// 키는 snake_case lowercase.
const HOOK_EVENTS: &[(&str, &str, &str)] = &[
    ("Stop", "stop", "stop"),
    ("UserPromptSubmit", "prompt-submit", "user_prompt_submit"),
    ("SessionStart", "session-start", "session_start"),
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
    // TASTY_SURFACE_ID 가 비어있을 때 skip 하는 guard 포함. 가드 없으면 codex 가
    // 변수를 빈 문자열로 치환해 `tasty codex hook X --surface ` 가 실행되어
    // invalid_params 노이즈 발생.
    //
    // Windows: codex 는 hook 명령을 PowerShell 로 실행한다(실측 2026-07-12 —
    // 단일따옴표/`#` 주석이 PS 규칙으로 해석되고 순수 PS 구문 명령이 성공).
    // POSIX `[ -n ... ]` 가드는 PS 파서에서 항상 실패해 hook 이 한 번도 성공하지
    // 못하므로 PS 구문으로 발행한다. stdin 의 payload JSON 은 `$input` 으로 tasty
    // CLI 에 그대로 전달한다(session_id 추출용).
    #[cfg(windows)]
    {
        format!(
            "if ($env:TASTY_SURFACE_ID) {{ $input | tasty codex hook {event_kebab} --surface $env:TASTY_SURFACE_ID }}"
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "[ -n \"$TASTY_SURFACE_ID\" ] && tasty codex hook {event_kebab} --surface $TASTY_SURFACE_ID || true"
        )
    }
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
    for (event_key, kebab, _trust_snake) in HOOK_EVENTS {
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

/// 우리가 install 한 3 개 hook 모두 trusted 상태인지 확인.
///
/// codex 는 user 가 `/hooks` 로 trust 한 hook 에 대해 `[hooks.state."<path>:<snake_event>:0:0"]`
/// 섹션에 `trusted_hash = "sha256:..."` 를 박는다. 우리 install entry 가 모두 그
/// 형식으로 등록되어있어야 hook 이 실제 fire 된다.
///
/// 주의: codex 는 부팅 시 stored hash 와 현재 hook command 의 fresh hash 를 비교해서
/// 다르면 invalidate 한다. 본 체크는 키 존재 + sha256: prefix 만 보므로, stale entry
/// 가 있고 codex 가 invalidate 한 케이스는 못 잡는다. 하지만 우리 install 은 멱등하고
/// `hook_command()` 가 static 이라 실제 stale 케이스는 사용자가 config.toml 을 직접
/// 편집한 경우 정도. `--dangerously-bypass-hook-trust`(기동 명령에 항상 포함)가
/// 이 여부와 무관하게 hook 을 fire 시키므로, 이 함수는 이제 `handle_install` 의
/// 안내 문구(수동 승인 상태 표시)에만 쓰인다 — 실제 hook 동작에는 영향 없음.
fn codex_hooks_all_trusted() -> bool {
    let Ok(path) = codex_config_toml_path() else {
        return false;
    };
    let value = read_toml_or_default(&path);
    codex_hooks_all_trusted_in(&value, &path.to_string_lossy())
}

fn codex_hooks_all_trusted_in(value: &toml::Value, source_path: &str) -> bool {
    let Some(state_table) = value
        .get("hooks")
        .and_then(|v| v.get("state"))
        .and_then(|v| v.as_table())
    else {
        return false;
    };
    for (_, _, trust_snake) in HOOK_EVENTS {
        let key = format!("{source_path}:{trust_snake}:0:0");
        let trusted = state_table
            .get(&key)
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("trusted_hash"))
            .and_then(|h| h.as_str())
            .map(|s| s.starts_with("sha256:") && s.len() > "sha256:".len())
            .unwrap_or(false);
        if !trusted {
            return false;
        }
    }
    true
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
        assert_eq!(
            make_codex_command(42, None),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust\r"
        );
        assert_eq!(
            make_codex_command(42, Some("")),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust\r"
        );
    }

    #[test]
    fn make_codex_command_with_plain_prompt() {
        assert_eq!(
            make_codex_command(42, Some("hello")),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust \"hello\"\r"
        );
    }

    #[test]
    fn make_codex_command_with_prompt_escapes_quotes() {
        let cmd = make_codex_command(7, Some(r#"fix "bug" please"#));
        assert_eq!(
            cmd,
            "TASTY_SURFACE_ID=7 codex --dangerously-bypass-hook-trust \"fix \\\"bug\\\" please\"\r"
        );
    }

    #[test]
    fn make_codex_command_with_prompt_escapes_backslash() {
        let cmd = make_codex_command(7, Some(r"path\to\file"));
        assert_eq!(
            cmd,
            "TASTY_SURFACE_ID=7 codex --dangerously-bypass-hook-trust \"path\\\\to\\\\file\"\r"
        );
    }

    #[test]
    fn hook_event_to_state_maps_known_events() {
        assert_eq!(hook_event_to_state("stop").unwrap(), "idle");
        assert_eq!(hook_event_to_state("prompt-submit").unwrap(), "active");
        assert_eq!(hook_event_to_state("session-start").unwrap(), "active");
    }

    #[test]
    fn hook_event_to_state_rejects_unsupported() {
        // notification / session-end / subagent-stop 은 codex 가 fire 하지 않으므로
        // 거부 (silent no-op 대신 invalid_params).
        let err = hook_event_to_state("notification").unwrap_err();
        assert!(format!("{err:?}").contains("unknown hook event"));
    }

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
        // threshold=6 이면 안 뜨는 3개가, threshold=3 이면 뜬다(설정 override 시나리오).
        assert_eq!(build_spawn_warning(3, &[], 6.0), None);
        assert!(build_spawn_warning(4, &[], 3.0).is_some());
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
        for (event_key, _, _) in HOOK_EVENTS {
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
        for (key, _, _) in HOOK_EVENTS {
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
    fn codex_hooks_all_trusted_in_returns_true_when_all_three_present() {
        let path = "/Users/x/.codex/config.toml";
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc123"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = "sha256:def456"

[hooks.state."{path}:session_start:0:0"]
trusted_hash = "sha256:fff999"
"#
        );
        let value = parse_toml(&toml);
        assert!(codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_any_missing() {
        let path = "/Users/x/.codex/config.toml";
        // Stop + UserPromptSubmit 만, SessionStart 빠짐.
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = "sha256:def"
"#
        );
        let value = parse_toml(&toml);
        assert!(!codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_hash_value_invalid() {
        let path = "/Users/x/.codex/config.toml";
        // 3 개 entry 모두 있지만 trusted_hash 가 sha256: prefix 미충족.
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = ""

[hooks.state."{path}:session_start:0:0"]
trusted_hash = "sha256:abc"
"#
        );
        let value = parse_toml(&toml);
        assert!(!codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_no_state_section() {
        let value = parse_toml("model = \"gpt-5.5\"");
        assert!(!codex_hooks_all_trusted_in(&value, "/x"));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_only_other_paths_present() {
        // 다른 config 경로의 hook 만 있는 경우 → 우리 경로 기준 false.
        let toml = r#"
[hooks.state."/other/path:stop:0:0"]
trusted_hash = "sha256:xyz"

[hooks.state."/other/path:user_prompt_submit:0:0"]
trusted_hash = "sha256:xyz"

[hooks.state."/other/path:session_start:0:0"]
trusted_hash = "sha256:xyz"
"#;
        let value = parse_toml(toml);
        assert!(!codex_hooks_all_trusted_in(
            &value,
            "/Users/x/.codex/config.toml"
        ));
    }

    // ── 형제 once-hook 정리 재현 (TODO 23) ──

    use std::cell::RefCell;

    struct MockHook {
        id: u64,
        surface_id: u32,
        command: String,
        event: String,
    }

    /// hook.set/list/unset + terminal.tell 을 in-memory 로 시뮬레이션하는 mock 호스트.
    /// `alive` 는 `surface.locate` 응답을 시뮬레이션 — 기본은 아무도 살아있지 않은
    /// 것으로 취급하고(= surface.locate 조회 실패와 동일하게 안전 쪽으로 fallback),
    /// `mark_alive`/`mark_dead` 로 명시적으로 상태를 세팅한다.
    struct MockHost {
        hooks: RefCell<Vec<MockHook>>,
        next_id: RefCell<u64>,
        alive: RefCell<std::collections::HashSet<u32>>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                hooks: RefCell::new(Vec::new()),
                next_id: RefCell::new(1),
                alive: RefCell::new(std::collections::HashSet::new()),
            }
        }

        /// event 발화 시뮬레이션 — 매칭 once-hook 제거(호스트 `check_and_fire` retain 동일).
        fn fire(&self, surface_id: u32, event: &str) -> usize {
            let mut hooks = self.hooks.borrow_mut();
            let before = hooks.len();
            hooks.retain(|h| !(h.surface_id == surface_id && h.event == event));
            before - hooks.len()
        }

        fn commands_on(&self, surface_id: u32) -> Vec<String> {
            self.hooks
                .borrow()
                .iter()
                .filter(|h| h.surface_id == surface_id)
                .map(|h| h.command.clone())
                .collect()
        }

        /// `surface.locate` 가 `exists: true` 를 돌려주도록(= 아직 process 가 살아있음).
        fn mark_alive(&self, surface_id: u32) {
            self.alive.borrow_mut().insert(surface_id);
        }

        /// `surface.locate` 가 `exists: false` 를 돌려주도록(= process-exit 로 host 가
        /// 이미 surface 를 닫아버림을 재현).
        fn mark_dead(&self, surface_id: u32) {
            self.alive.borrow_mut().remove(&surface_id);
        }
    }

    impl HostCall for MockHost {
        fn call(
            &self,
            method: &str,
            params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            match method {
                "hook.set" => {
                    let mut id = self.next_id.borrow_mut();
                    let hid = *id;
                    *id += 1;
                    self.hooks.borrow_mut().push(MockHook {
                        id: hid,
                        surface_id: params["surface_id"].as_u64().unwrap() as u32,
                        command: params["command"].as_str().unwrap().to_string(),
                        event: params["event"].as_str().unwrap().to_string(),
                    });
                    Ok(json!({ "hook_id": hid }))
                }
                "hook.list" => {
                    let sid = params["surface_id"].as_u64().map(|v| v as u32);
                    let arr: Vec<Value> = self
                        .hooks
                        .borrow()
                        .iter()
                        .filter(|h| sid.is_none_or(|s| h.surface_id == s))
                        .map(|h| {
                            json!({ "id": h.id, "surface_id": h.surface_id, "command": h.command, "event": h.event })
                        })
                        .collect();
                    Ok(json!(arr))
                }
                "hook.unset" => {
                    let hid = params["hook_id"].as_u64().unwrap();
                    self.hooks.borrow_mut().retain(|h| h.id != hid);
                    Ok(json!({ "removed": true }))
                }
                "surface.locate" => {
                    let sid = params["surface_id"].as_u64().unwrap() as u32;
                    let exists = self.alive.borrow().contains(&sid);
                    Ok(json!({ "surface_id": sid, "exists": exists }))
                }
                _ => Ok(json!({})),
            }
        }
    }

    #[test]
    fn siblings_to_unset_isolates_by_command() {
        let spawn_cmd = notify_caller_command(9, 100, "spawn");
        let tell_cmd = notify_caller_command(9, 100, "tell");
        let hooks = vec![
            json!({ "id": 1, "command": spawn_cmd, "event": "process-exit" }),
            json!({ "id": 2, "command": tell_cmd, "event": "process-exit" }),
            json!({ "id": 3, "command": spawn_cmd, "event": "codex-idle" }),
        ];
        assert_eq!(siblings_to_unset(&hooks, &spawn_cmd), vec![1, 3]);
    }

    // ── 완료 알림 문구 (TODO 07: "spawn 완료" 오독 방지) ──

    #[test]
    fn notify_caller_message_leads_with_work_completion() {
        let msg = notify_caller_message("spawn", 42);
        assert!(
            msg.contains("작업 완료"),
            "완료 대상이 '작업'임이 드러나야 함: {msg}"
        );
        assert!(msg.contains("42"), "target surface 번호 누락: {msg}");
        assert!(msg.contains("spawn"), "호출 방식 정보 누락: {msg}");
    }

    #[test]
    fn notify_caller_message_does_not_read_as_command_itself_completing() {
        // 회귀 방지: 과거 "{kind} 완료: surface N" 형태는 "spawn 이라는 동작이
        // 완료됐다"로 오독되기 쉬웠다 — kind 가 더 이상 완료의 주어로 문장 맨 앞에
        // 오지 않아야 한다.
        for kind in ["spawn", "tell"] {
            let msg = notify_caller_message(kind, 7);
            assert!(
                !msg.starts_with(&format!("{kind} 완료")),
                "옛 오독 유발 포맷으로 회귀함: {msg}"
            );
        }
    }

    #[test]
    fn sibling_cleanup_removes_all_after_one_fires() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 2, "2 형제 등록");

        // codex-idle 이 fire(once 제거) → 나머지 형제(process-exit) 정리.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        let expected = notify_caller_command(caller, target, "tell");
        cleanup_sibling_hooks(&host, target, &expected);

        assert!(
            host.commands_on(target).is_empty(),
            "형제 hook 이 하나도 남지 않아야 함 — process-exit 좀비 없음: {:?}",
            host.commands_on(target)
        );
    }

    #[test]
    fn concurrent_registrations_leave_no_zombie() {
        // 같은 child(target) 에 spawn 완료 hook 과 tell 완료 hook 이 겹쳐 등록된 상태.
        // 옛 단일 meta 슬롯(`codex-notify-hooks`) 방식이면 tell 등록이 spawn 의 sibling
        // id 목록을 덮어써, spawn 그룹의 process-exit 이 정리되지 못하고 좀비로 남았다.
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, target, caller, "spawn");
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 4, "두 그룹 = 4 hook");

        // spawn 그룹의 codex-idle 이 먼저 fire → spawn 그룹만 정리.
        host.fire(target, "codex-idle");
        let spawn_cmd = notify_caller_command(caller, target, "spawn");
        cleanup_sibling_hooks(&host, target, &spawn_cmd);

        let remaining = host.commands_on(target);
        let tell_cmd = notify_caller_command(caller, target, "tell");
        assert!(
            remaining.iter().all(|c| c == &tell_cmd),
            "spawn 그룹 좀비 잔존: {remaining:?}"
        );
        assert!(
            !remaining.iter().any(|c| c == &spawn_cmd),
            "spawn 그룹 process-exit 좀비 남음"
        );

        // 이제 tell 그룹도 fire → 전부 정리.
        host.fire(target, "process-exit");
        cleanup_sibling_hooks(&host, target, &tell_cmd);
        assert!(
            host.commands_on(target).is_empty(),
            "최종적으로 형제 hook 이 전부 사라져야 함: {:?}",
            host.commands_on(target)
        );
    }

    // ── 자기재무장(self-rearm) — child 가 살아있는 동안 알림 반복 (TODO 08) ──
    //
    // 배경: codex-idle 은 process-exit 와 달리 "child 가 아직 살아있는 상태 전환"일 수
    // 있다. 형제 hook 이 once=true 라 한 번 fire 하면 남은 형제도 정리돼 그 spawn/tell
    // 콜당 알림이 딱 1번만 오던 문제 — 진짜 완료 전에 codex-idle 을 한 번이라도 거치면
    // 그 뒤엔 재알림 경로가 없었다.

    #[test]
    fn handle_notify_caller_rearms_when_target_still_alive() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 2, "최초 2 형제 등록");

        // 1번째 전환: codex-idle — child 는 여전히 살아있다.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            json!({ "caller": caller, "target": target, "kind": "tell" }),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            2,
            "살아있으면 형제 hook 이 다시 2개로 재무장돼야 함"
        );

        // 2번째 전환에도 계속 재무장되는지 확인 — 'spawn/tell 당 1회' 로 되돌아가면 안 됨.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            json!({ "caller": caller, "target": target, "kind": "tell" }),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            2,
            "두 번째 전환에도 재무장돼야 함"
        );
    }

    #[test]
    fn handle_notify_caller_does_not_rearm_when_target_exited() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, target, caller, "spawn");

        // process-exit 로 fire — host 는 이 시점에 이미 동기로 surface 를 닫으므로
        // surface.locate 가 exists:false 를 돌려주는 상황을 재현.
        assert_eq!(host.fire(target, "process-exit"), 1);
        host.mark_dead(target);
        handle_notify_caller(
            &host,
            json!({ "caller": caller, "target": target, "kind": "spawn" }),
        )
        .unwrap();

        assert!(
            host.commands_on(target).is_empty(),
            "죽은 surface 에 재무장하면 좀비 hook: {:?}",
            host.commands_on(target)
        );
    }
}
