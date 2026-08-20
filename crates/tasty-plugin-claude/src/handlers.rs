//! `tasty-claude` 의 IPC handler fn 들 — 외부 plugin SDK 진입점.
//!
//! 자식 terminal 관리(spawn/tell/wait/children/parent/kill/respawn/broadcast)는
//! 호스트가 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 **위임**한다. 이
//! plugin 은 더 이상 자체 child registry 를 보유하지 않는다(호스트 registry 가 단일
//! SoT). claude **특화**만 여기 남는다:
//! - `start_claude_in_surface` / `issue_session_token` — session token + agent id +
//!   TASTY_SURFACE_ID inline env 를 박은 기동 명령 (spawn/respawn 이 소비).
//! - `build_launch_command` / `handle_launch` — 새 workspace 기동 + error_scan 등록
//!   (top-level). `handle_spawn`/`handle_respawn` 은 자식 surface 를 error_scan 에
//!   등록하고 `handle_kill` 은 내린다.
//! - `handle_children` — 호스트 registry 목록에 `surface.foreground_process` 를 덧씌움.
//! - wall-time 텔레메트리 타이밍(`ClaudeState`) — hook.rs 가 소비.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::error_scan::{ErrorScanner, ScanTarget};

/// `profile_file`(직접 지정한 경로)과 `profile`(레지스트리에 등록된 이름 목록,
/// 쉼표 구분 — `profile.rs` 참고) params 를 최종 `--settings` 파일 경로 하나로
/// 해석한다. 둘 다 주어지면
/// 어느 쪽이 이기는지 조용히 정하지 않고 즉시 거부한다 — last-wins 함정을
/// 경로/이름 인자 사이에서도 반복하지 않기 위함.
pub(crate) fn resolve_profile_file_param(
    data_dir: Option<&Path>,
    params: &Value,
    tr: &Translator,
) -> Result<Option<String>, IpcMethodError> {
    let path = params
        .get("profile_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let names = params
        .get("profile")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    match (path, names) {
        (Some(_), Some(_)) => Err(IpcMethodError::new(
            tr.t("claude.profile.mutually_exclusive_file_and_profile"),
        )),
        (Some(p), None) => Ok(Some(p.to_string())),
        (None, Some(n)) => crate::profile::resolve_names(data_dir, n)
            .map(|p| Some(p.to_string_lossy().into_owned()))
            .map_err(|e| crate::profile::to_ipc_err(e, tr)),
        (None, None) => Ok(None),
    }
}

pub(crate) fn require_surface_id(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_surface_id")))
}

fn require_child_index(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    params
        .get("child_index")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_child_index")))
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

/// `hook.list` 응답 배열에서 정리 대상 형제 hook 의 id 들을 고른다 — command 문자열이
/// `expected_command` 와 정확히 일치하는 hook 만. 상태를 공유하지 않는(clobber 불가)
/// 순수 선택 로직이라 concurrent 등록에도 그룹 격리가 성립한다: 같은 target surface 에
/// 서로 다른 command(예: `--command spawn` vs `--command tell`)로 등록된 두 그룹은
/// 서로의 정리 대상에 포함되지 않는다.
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
                // remap 이 화이트리스트라 호스트가 실어 보낸 판정 근거 3 축 중
                // `state` 만 옮기면 나머지 둘이 여기서 잘린다 — `confidence` 가
                // 없으면 소비자가 확정 판정과 휴리스틱을 구분할 수 없다(ADR-0072).
                "evidence": c.get("evidence").cloned().unwrap_or(Value::Null),
                "confidence": c.get("confidence").cloned().unwrap_or(Value::Null),
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

/// 자식 Claude 를 종료한다 — 호스트 `terminal.kill` 로 위임(surface.close +
/// soft 점유 해제 + registry 제거). `child_index` → 호스트 `child` 매핑.
/// 종료된 surface 는 error scanner 에서도 즉시 내린다.
pub(crate) fn handle_kill(
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let child_index = require_child_index(params, tr)?;
    let mut kp = forward(params, &["surface"]);
    kp.insert("child".into(), json!(child_index));
    // 호스트 terminal.kill 성공 시 { killed_surface_id, child_index } 반환. claude
    // CLI 는 기존에 { killed: true } 를 기대하므로 성공을 그 shape 으로 변환한다.
    let resp = host_call(host, "terminal.kill", Value::Object(kp))?;
    // 폴링 루프의 생존 대조가 최대 800ms 뒤 어차피 정리하지만, 여기서 즉시 내리면
    // 그 사이 마지막 출력에 대고 `claude-error` 를 발화할 여지가 사라진다.
    if let Some(killed) = resp
        .get("killed_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        && let Ok(mut s) = scanner.lock()
    {
        s.disable(killed);
    }
    Ok(json!({ "killed": true }))
}

/// 부모의 모든(또는 role 필터된) 자식에 텍스트를 broadcast — 호스트
/// `terminal.broadcast` 로 위임.
pub(crate) fn handle_broadcast(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_text")))?;
    let mut bp = forward(params, &["surface", "role"]);
    bp.insert("text".into(), json!(text));
    host_call(host, "terminal.broadcast", Value::Object(bp))
}

/// 자식 Claude 에 메시지를 보낸다 — 호스트 `terminal.tell` 로 위임. 개행/제출
/// 규칙(단일라인 평문 / 멀티라인 bracketed paste + 별도 `\r`)은 호스트가 동일하게
/// 처리하므로 본문 포맷을 재구현하지 않는다.
pub(crate) fn handle_tell(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface_id(params, tr)?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_message")))?;
    let resp = host_call(
        host,
        "terminal.tell",
        json!({ "surface": surface_id, "text": message }),
    )?;

    // caller_surface 는 dynamic.rs 의 TASTY_SURFACE_ID 자동 채움으로 대개 채워진다.
    // 없으면(구버전 클라이언트 등) 어디로 알릴지 알 수 없으므로 알림 배선을
    // 생략한다(soft) — tell 자체의 성공/실패에는 영향 없음.
    if let Some(caller_surface) = params.get("caller_surface").and_then(|v| v.as_u64()) {
        register_notify_hooks(host, caller_surface as u32, surface_id, "tell");
    }

    Ok(resp)
}

/// caller_surface 에 spawn/tell 완료를 알리는 command 문자열 — 등록 시점과
/// 정리 시점에 동일하게 재생성해야 하므로 순수 함수로 분리한다. 3개 형제 hook이
/// 전부 이 문자열을 그대로 command 로 쓰므로, 서로의 hook_id 를 몰라도
/// (target_surface, command 일치) 기준으로 서로를 찾아 정리할 수 있다.
fn notify_done_command(caller_surface: u32, target_surface: u32, command_name: &str) -> String {
    format!(
        "tasty claude notify-done --caller-surface {caller_surface} --target-surface {target_surface} --command {command_name}"
    )
}

/// caller 에게 보여줄 완료 알림 문구 — "그 child 가 맡은 작업이 끝났다"를 앞세운다.
/// 과거 `"{command_name} 완료: surface {target_surface}"` 형태는 spawn/tell 자체가
/// (호출이) 완료됐다는 뜻으로 오독되기 쉬워, conductor 가 실제 작업 완료 알림을
/// "spawn 접수 확인" 정도로 여기고 계속 무시하는 사고로 이어졌다. `command_name`은
/// 호출 방식(spawn/tell)일 뿐 완료의 주어가 아니므로 괄호로 분리한다.
fn notify_done_message(tr: &Translator, command_name: &str, target_surface: u32) -> String {
    tr.t("claude.notify.done_message")
        .replacen("{}", &target_surface.to_string(), 1)
        .replacen("{}", command_name, 1)
}

/// spawn/tell 완료 시 caller 에게 1회성으로 알려줄 3개의 형제 hook
/// (claude-idle / needs-input / process-exit) 을 target_surface 에 등록한다.
/// 등록 자체는 best-effort — 실패해도 spawn/tell 성공 자체를 막지 않는다.
///
/// 에러 축([`register_error_notify_hook`])은 이 형제 그룹에 **넣지 않는다** —
/// 수명이 다르기 때문이다. 상태 전환 hook 은 "전환했으니 알리고 그룹째 정리 후
/// 재무장" 하는 once 사이클을 도는데, 에러 정지는 그 사이클과 무관하게 반복될 수
/// 있어 같은 그룹에 끼우면 서로의 정리 대상이 되어 사이클이 꼬인다.
fn register_notify_hooks<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
    command_name: &str,
) {
    let command = notify_done_command(caller_surface, target_surface, command_name);
    for event in ["claude-idle", "needs-input", "process-exit"] {
        // best-effort — 등록 실패해도 spawn/tell 자체는 이미 성공했으므로 무시.
        let _ = host.call(
            "hook.set",
            json!({
                "surface_id": target_surface,
                "event": event,
                "command": command,
                "once": true,
            }),
        );
    }
    register_error_notify_hook(host, caller_surface, target_surface);
}

/// 에러 정지 알림 hook 의 command 문자열. 완료 알림([`notify_done_command`])과 **다른**
/// 문자열이라 `cleanup_sibling_hooks` 의 정리 대상(command 완전 일치)에 걸리지 않는다 —
/// 형제 그룹이 fire·정리·재무장을 반복해도 이 hook 은 건드려지지 않는다.
fn notify_error_command(caller_surface: u32, target_surface: u32) -> String {
    format!(
        "tasty claude notify-error --caller-surface {caller_surface} --target-surface {target_surface}"
    )
}

/// `claude-error-stalled` 를 구독하는 **상시**(once 아님) hook 을 등록한다.
///
/// once 가 아닌 이유: 한 번 알리고 사라지면 그 뒤의 정지는 놓치는데, 재무장을 붙이면
/// 형제 그룹의 once 사이클을 그대로 복제해야 한다. 상시 hook 이면 재무장 자체가
/// 필요 없고, 발사 빈도 상한은 발신 측(`error_scan.rs` 의 쿨다운·에피소드 1회)이
/// 이미 갖고 있다.
///
/// 등록은 멱등하다 — spawn 후 tell, 그리고 형제 재무장까지 이 함수를 여러 번 부르므로
/// 같은 command 의 기존 hook 을 먼저 걷어내고 새로 단다. 걷어내지 않으면 같은 정지에
/// 알림이 등록 횟수만큼 중복된다.
fn register_error_notify_hook<H: HostCall>(host: &H, caller_surface: u32, target_surface: u32) {
    let command = notify_error_command(caller_surface, target_surface);
    cleanup_sibling_hooks(host, target_surface, &command);
    // best-effort — 등록 실패해도 spawn/tell 자체는 이미 성공했으므로 무시.
    let _ = host.call(
        "hook.set",
        json!({
            "surface_id": target_surface,
            "event": crate::error_scan::STALLED_EVENT,
            "command": command,
            "once": false,
        }),
    );
}

/// `tasty claude notify-done` — 형제 once-hook 중 하나가 fire 되어 실행되는
/// 커맨드. caller_surface 에 완료 메시지를 주입한 뒤, target_surface 에 남아있는
/// (아직 fire 되지 않은) 나머지 형제 hook 들을 command 문자열 일치로 찾아 정리한다.
pub(crate) fn handle_notify_done<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let caller_surface = params
        .get("caller_surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(tr.t("claude.params.missing_caller_surface"))
        })? as u32;
    let target_surface = params
        .get("target_surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(tr.t("claude.params.missing_target_surface"))
        })? as u32;
    let command_name = params
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_command")))?;

    // 1) caller 에게 알림 주입.
    let message = notify_done_message(tr, command_name, target_surface);
    // 완료 로그 파일에 append — conductor 가 Monitor tool 로 tail 하면 busy/idle
    // 여부와 무관하게 다음 턴에 전달된다. 완료 알림의 유일한 경로다(과거엔
    // terminal.tell 도 함께 발사했으나, 자동 이벤트가 실제 사용자 발화처럼 대화
    // 트랜스크립트에 섞여 들어가는 부작용 때문에 제거함). best-effort — 실패해도
    // 형제 hook 정리에 영향 없음.
    if let Err(e) = tasty_utils::notify::append_notify_line(caller_surface, &message) {
        tracing::warn!("claude notify-done completion-log append failed: {e}");
    }

    // 2) 남은 형제 hook 정리 — 나 자신은 이미 once=true 로 fire 시 자동 제거됐으므로
    //    hook.list 시점엔 나머지(0~2개)만 남아있다. command 문자열 완전 일치로 식별하되
    //    반드시 target_surface 로 필터해 다른 child(=다른 surface)의 hook 은 건드리지
    //    않는다. 상태 없는(clobber 불가) 순수 선택이라 concurrent 등록에도 안전.
    let expected_command = notify_done_command(caller_surface, target_surface, command_name);
    cleanup_sibling_hooks(host, target_surface, &expected_command);

    // 3) target_surface 가 아직 살아있다면(이번 fire 가 process-exit 가 아니었다면)
    //    형제 hook 을 다시 3개 등록해 다음 idle/needs-input 전환에도 알림이 오도록
    //    자기재무장한다 — "spawn/tell 당 알림 1회" 가 아니라 "child 가 살아있는 동안
    //    상태 전환마다 알림"으로 바뀐다. claude-idle/needs-input 은 일시적 상태 전환일
    //    수 있어(예: 애매한 지시에 되묻고 다시 작업 재개) 여기서 멈추면 진짜 완료를
    //    영영 놓친다.
    rearm_if_still_alive(host, caller_surface, target_surface, command_name);

    Ok(json!({}))
}

/// `tasty claude notify-error` — `claude-error-stalled` 상시 hook 이 fire 되어 실행되는
/// 커맨드. caller_surface 의 알림 로그에 "자식이 에러 후 멈췄다" 를 append 한다.
///
/// 완료 알림(`notify-done`)과 달리 **형제 정리도 재무장도 하지 않는다** — 상시 hook 이라
/// 그대로 남아 다음 정지도 받는다. 상태 축도 건드리지 않는다(`terminal.set_state` 미호출):
/// 에러는 재시도로 복구될 수 있어 상태로 승격하면 오탐이 되고, 파생 상태는 관측 융합의
/// 출력 전용 계약이다(`docs/adr/0072-child-state-hook-observation-fusion.md`).
pub(crate) fn handle_notify_error<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let caller_surface = params
        .get("caller_surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(tr.t("claude.params.missing_caller_surface"))
        })? as u32;
    let target_surface = params
        .get("target_surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(tr.t("claude.params.missing_target_surface"))
        })? as u32;

    let message = notify_error_message(tr, host, target_surface);
    if let Err(e) = tasty_utils::notify::append_notify_line(caller_surface, &message) {
        tracing::warn!("claude notify-error completion-log append failed: {e}");
    }
    Ok(json!({}))
}

/// 정지 알림 문구 — 알림 조립 직전 대상 화면을 읽어 에러 줄을 힌트로 덧붙인다(codex
/// `notify-caller` 와 같은 방식). 화면 조회는 best-effort 라, 실패하면 힌트 없이
/// 본문만 보낸다.
fn notify_error_message<H: HostCall>(tr: &Translator, host: &H, target_surface: u32) -> String {
    let mut message =
        tr.t("claude.notify.stalled_message")
            .replacen("{}", &target_surface.to_string(), 1);
    let screen = host
        .call(
            "surface.screen_text",
            json!({ "surface_id": target_surface }),
        )
        .ok()
        .and_then(|r| r.get("text").and_then(|t| t.as_str()).map(str::to_string));
    if let Some(line) = screen
        .as_deref()
        .and_then(crate::error_scan::first_error_line)
    {
        // 화면 한 줄이 그대로 알림에 실린다 — 로그 한 줄 형식을 깨지 않도록 길이를 자른다.
        let hint: String = line.chars().take(160).collect();
        message.push_str(&tr.t("claude.notify.stalled_hint").replacen("{}", &hint, 1));
    }
    message
}

/// `target_surface` 가 host 트리에 여전히 존재하면(=이번 fire 가 process-exit 가
/// 아니었다면) 형제 hook 3개를 재등록한다. `surface.locate` 로 생존을 판별하는 이유:
/// process-exit 로 fire 된 경우 host 는 hook 발화 직후 동기로 그 surface 를 이미
/// 닫으므로(`close_surface_by_id_no_snapshot`), 이 시점에 조회하면 사라져 있다 —
/// 반대로 claude-idle/needs-input 은 surface 가 살아있는 상태에서만 발생하는
/// 이벤트라 재등록이 안전하다. 조회 실패(best-effort)는 "죽었다"로 간주해 재등록을
/// 건너뛴다 — 좀비 hook 을 쌓는 것보다 드물게 재무장을 놓치는 쪽이 안전하다.
fn rearm_if_still_alive<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
    command_name: &str,
) {
    let alive = host
        .call("surface.locate", json!({ "surface_id": target_surface }))
        .ok()
        .and_then(|r| r.get("exists").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if alive {
        register_notify_hooks(host, caller_surface, target_surface, command_name);
    }
}

/// 새 workspace 를 만들고 그 안에서 claude 를 기동한다. child 가 아니라 top-level
/// 이므로 호스트 child registry 에 등록하지 않는다(launch 는 05 범위 밖 특화 잔류).
/// error scanner 에 그 surface 를 등록한다.
pub(crate) fn handle_launch(
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
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
    let profile_file = resolve_profile_file_param(data_dir, params, tr)?;

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
        .ok_or_else(|| IpcMethodError::new(tr.t("claude.launch.workspace_create_missing_id")))?;
    let surface_id = ws_resp
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if let Some(sid) = surface_id {
        let cmd = build_launch_command(task.as_deref(), profile_file.as_deref());
        if let Err(e) = host.call(
            "surface.send",
            json!({ "surface_id": sid, "text": format!("{cmd}\r") }),
        ) {
            tracing::warn!("surface.send (launch) failed: {e}");
        }

        if let Ok(mut s) = scanner.lock() {
            s.enable(sid, ScanTarget::TopLevel);
        }
    }

    Ok(json!({
        "workspace_id": workspace_id,
        "workspace_name": workspace_name,
        "surface_id": surface_id,
    }))
}

/// `claude` / `claude --task <escaped>` / 뒤에 `--settings "<path>"` 가 붙는 조합.
/// `profile_file` 은 CLI `path_kind = "file"` 정규화를 이미 거친 절대경로 — 인라인
/// JSON 이 아니라 파일 경로를 큰따옴표로 감싼다(`reboot::resume_command` 와 동일 규칙).
pub(crate) fn build_launch_command(task: Option<&str>, profile_file: Option<&str>) -> String {
    let mut cmd = "claude".to_string();
    if let Some(t) = task {
        let escaped = shell_escape::escape(t.into());
        cmd.push_str(&format!(" --task {escaped}"));
    }
    if let Some(path) = profile_file {
        cmd.push_str(&format!(" --settings \"{path}\""));
    }
    cmd
}

/// 자식 surface 의 PTY 를 갈아끼우고 claude 를 재시작한다. registry 조작(PTY 교체/
/// Ctrl-C + metadata 갱신 + idle 초기화)은 호스트 `terminal.respawn` 으로 위임하고,
/// claude 특화 기동 명령만 그 위에 재전송한다. `child_index` → 호스트 `child` 매핑.
/// error scanner 에는 (재)등록 + dedupe 초기화한다 — surface_id 가 유지되므로 이전
/// 인스턴스가 남긴 dedupe 스니펫이 재기동 후 같은 에러를 억제할 수 있다.
pub(crate) fn handle_respawn(
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params, tr)?;
    let child_index = require_child_index(params, tr)?;
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);
    let profile_file = resolve_profile_file_param(data_dir, params, tr)?;

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
            IpcMethodError::new(
                tr.t_fmt("claude.respawn.missing_child_surface_id", &resp.to_string()),
            )
        })?;

    // 2) claude 특화 기동 명령 재전송.
    start_claude_in_surface(
        host,
        child_surface_id,
        prompt.as_deref(),
        profile_file.as_deref(),
    );

    // 3) error scan 대상으로 (재)등록. PTY 가 갈렸으므로 이전 인스턴스의 dedupe
    //    스니펫은 버린다 — 안 버리면 재기동 후 같은 에러 텍스트가 다시 나도
    //    `claude-error` 가 억제된다.
    if let Ok(mut s) = scanner.lock() {
        s.enable(child_surface_id, ScanTarget::Child);
        s.reset_dedupe(child_surface_id);
    }

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
pub(crate) fn start_claude_in_surface(
    host: &HostHandle,
    surface_id: u32,
    prompt: Option<&str>,
    profile_file: Option<&str>,
) {
    let agent_id = format!("claude_s{surface_id}");
    let session_token = issue_session_token(host, &agent_id);
    let agent_prefix = match session_token {
        Some(tok) => format!(
            "TASTY_SURFACE_ID={surface_id} TASTY_AGENT_ID={agent_id} TASTY_SESSION_TOKEN={tok} "
        ),
        None => format!("TASTY_SURFACE_ID={surface_id} TASTY_AGENT_ID={agent_id} "),
    };
    let text = match prompt {
        Some(p) => claude_launch_command_with_prompt(surface_id, &agent_prefix, p, profile_file),
        None => match profile_file {
            Some(path) => format!("{agent_prefix}claude --settings \"{path}\"\r"),
            None => format!("{agent_prefix}claude\r"),
        },
    };

    if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": text }),
    ) {
        tracing::warn!("surface.send (claude) failed: {e}");
    }
}

/// prompt 임시파일 이름 prefix/suffix — 정리 스윕(`sweep_stale_prompt_files`)이 같은
/// 패턴으로 자기 파일만 매칭하도록 상수로 뽑아 공유한다.
const PROMPT_FILE_PREFIX: &str = "tasty-prompt-";
const PROMPT_FILE_SUFFIX: &str = ".txt";

/// prompt 임시파일이 생성된 뒤 이만큼 지나면 다음 spawn 시점에 청소 대상이 된다.
/// 자식 셸이 `$(cat '<path>')` 치환을 끝내는 데는 보통 수 ms~수 초면 충분하므로,
/// 10분이면 그 시간을 넉넉히 넘겨 "아직 안 읽었는데 지워지는" 레이스를 사실상
/// 배제한다.
const PROMPT_FILE_TTL: std::time::Duration = std::time::Duration::from_secs(600);

/// prompt 를 임시 파일에 쓰고 `$(cat ...)` 로 주입하는 claude 기동 명령을 만든다.
/// 파일 쓰기 실패는 warn 후에도 계속 진행한다(빈 프롬프트로라도 기동은 시도).
/// `profile_file` 이 있으면 positional prompt 인자보다 앞에 `--settings "<path>"` 를
/// 붙인다. 이 함수는 이미 POSIX 전용 env prefix 를 쓰고 있어(`agent_prefix`) 그
/// 플랫폼 정합은 이 변경의 범위 밖 — 기존 상태를 그대로 따른다.
///
/// 파일 정리 시점: 자식이 `$(cat ...)` 로 이 파일을 다 읽은 순간을 tasty 가 알
/// 방법이 없다(`surface.send` 는 fire-and-forget 텍스트 주입) — 쓰자마자 지우면
/// 아직 안 읽은 자식과 레이스한다. 대신 매 spawn 마다 [`PROMPT_FILE_TTL`] 을 넘긴
/// 이전 파일들을 먼저 청소한다(`sweep_stale_prompt_files`) — 지연 삭제.
/// 권한은 생성 시점부터 0600(owner-only, Unix) 으로 좁힌다 — 생성 후 별도
/// `chmod` 로 좁히면 그 사이 기본 권한(보통 0644)으로 잠깐 노출되는 TOCTOU 창이
/// 생기므로, `OpenOptions`(Unix `mode`)로 처음부터 좁게 만든다.
fn claude_launch_command_with_prompt(
    surface_id: u32,
    agent_prefix: &str,
    prompt: &str,
    profile_file: Option<&str>,
) -> String {
    let temp_dir = std::env::temp_dir();
    sweep_stale_prompt_files(&temp_dir);
    let prompt_path = temp_dir.join(format!(
        "{PROMPT_FILE_PREFIX}{surface_id}{PROMPT_FILE_SUFFIX}"
    ));
    if let Err(e) = write_prompt_file(&prompt_path, prompt) {
        tracing::warn!("Failed to write prompt file: {e}");
    }
    let settings_flag = match profile_file {
        Some(path) => format!("--settings \"{path}\" "),
        None => String::new(),
    };
    format!(
        "{agent_prefix}claude {settings_flag}\"$(cat '{}')\"\r",
        prompt_path.display()
    )
}

/// `path` 를 owner-only(0600) 권한으로 새로 만들어 `content` 를 쓴다. 같은 경로에
/// 이전 실행이 남긴 파일이 있으면(과거 버전이 좁히지 않은 권한으로 만들었을 수
/// 있는 파일 포함) 먼저 지우고 다시 만든다 — `OpenOptions::mode` 는 파일을 실제로
/// *생성*하는 순간에만 적용되고, 이미 존재하는 파일을 여는 경우엔 기존 권한이
/// 그대로 유지되기 때문이다(재사용 시 권한이 좁혀지지 않는 구멍을 막는다).
fn write_prompt_file(path: &Path, content: &str) -> std::io::Result<()> {
    // 없는 파일을 지우려는 실패(가장 흔한 경우 — 이전 실행 잔재가 없음)를 포함해
    // 결과를 신경 쓰지 않는다: 존재하든 안 하든 바로 이어지는 `create(true)` 가
    // 최종적으로 원하는 상태(0600 새 파일)를 만든다.
    let _ = std::fs::remove_file(path);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        // Windows 는 Unix mode 개념이 없다 — ACL 기반 권한 좁히기는 이 변경의
        // 범위 밖(별도 작업 필요), 기존 `std::fs::write` 기본 동작 그대로 둔다.
        std::fs::write(path, content)
    }
}

/// `dir` 안에서 `PROMPT_FILE_PREFIX`/`PROMPT_FILE_SUFFIX` 패턴에 매칭하고
/// `PROMPT_FILE_TTL` 보다 오래된(mtime 기준) 파일을 best-effort 로 지운다 —
/// 매 spawn 마다 호출되는 지연 삭제 방식(무기한 누적 방지). 읽기/삭제 실패는
/// 무시한다: 디렉터리 목록 경합, 동시에 다른 프로세스가 아직 참조 중인 파일 등은
/// spawn 자체를 막을 이유가 아니다 — 대신 `tracing::debug!` 로 남긴다.
fn sweep_stale_prompt_files(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "prompt tempfile sweep: read_dir({}) failed: {e}",
                dir.display()
            );
            return;
        }
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(PROMPT_FILE_PREFIX) || !name.ends_with(PROMPT_FILE_SUFFIX) {
            continue;
        }
        let is_stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .and_then(|modified| {
                now.duration_since(modified)
                    .map_err(|e| std::io::Error::other(e.to_string()))
            })
            .is_ok_and(|age| age >= PROMPT_FILE_TTL);
        if is_stale && let Err(e) = std::fs::remove_file(entry.path()) {
            tracing::debug!(
                "prompt tempfile sweep: remove({:?}) failed: {e}",
                entry.path()
            );
        }
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
/// 자식 surface 는 error scanner 대상으로 등록한다 — 사람이 보고 있지 않은 자식이야말로
/// 네트워크/API 에러 감지가 가장 필요한 대상이다.
pub(crate) fn handle_spawn(
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let parent_surface_id = require_surface_id(params, tr)?;
    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(String::from);
    let profile_file = resolve_profile_file_param(data_dir, params, tr)?;

    // 1) 호스트 registry 에 등록 + 점유 + tab 생성. workspace required.
    let mut sp = forward(params, &["workspace", "pane", "cwd", "role", "nickname"]);
    sp.insert("parent".into(), json!(parent_surface_id));
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child_surface_id = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(
                tr.t_fmt("claude.spawn.missing_child_surface_id", &resp.to_string()),
            )
        })?;

    // 2) claude 특화 기동 명령 전송(session token + surface_id inline env 필요).
    start_claude_in_surface(
        host,
        child_surface_id,
        prompt.as_deref(),
        profile_file.as_deref(),
    );

    // 2-1) error scan 대상 등록. `ScanTarget::Child` 로 넣으면 폴링 루프가
    //      `terminal.parent` 로 관계 생존을 대조하므로, kill/close 뿐 아니라
    //      `terminal.release`(surface 는 남기고 관계만 해제)까지 함께 정리된다.
    if let Ok(mut s) = scanner.lock() {
        s.enable(child_surface_id, ScanTarget::Child);
    }

    // claude CLI/auto_wait 는 응답에 parent_surface_id 를 기대한다(호스트 응답엔
    // 없으므로 caller surface 로 채운다). 나머지 필드(child_surface_id/child_index/
    // pane_id/workspace_id)는 호스트 응답 그대로.
    let mut out = resp;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("parent_surface_id".into(), json!(parent_surface_id));
    }

    // 3) child 개수 임계치 경고(soft) — spawn 자체를 막지 않는다.
    if let Some(warning) = compute_spawn_warning(host, parent_surface_id, tr) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("warning".into(), json!(warning));
        }
    }

    // 4) caller(parent_surface_id)에게 완료(idle/needs_input/exited) 1회성 알림 배선.
    register_notify_hooks(host, parent_surface_id, child_surface_id, "spawn");

    Ok(out)
}

const DEFAULT_SPAWN_CHILD_WARN_THRESHOLD: f64 = 6.0;

/// spawn 직후 parent 의 현재 child 목록/상태를 재조회해 임계치 초과 여부를 판단한다.
/// host 호출 실패는 경고 생략으로 처리한다(soft 경고이므로 spawn 성공을 막지 않음).
///
/// 여기서 부르는 건 claude 특화 remap 된 `claude.children`(필드명 `child_surface_id`)이
/// 아니라 **원본** `terminal.children`(필드명 `surface_id`) — `index`/`state` 필드명은
/// 양쪽 shape 모두 동일하므로 아래 파싱 코드는 원본 응답에 그대로 맞는다.
fn compute_spawn_warning(
    host: &HostHandle,
    parent_surface_id: u32,
    tr: &Translator,
) -> Option<String> {
    let children_resp = host
        .call("terminal.children", json!({ "surface": parent_surface_id }))
        .ok()?;
    let children = children_resp.get("children")?.as_array()?;
    let total = children.len();
    let idle_indices = indices_with(children, |c| state_of(c) == Some("idle"));
    // `stale` 은 확정(`foreground_is_shell`)인 것만 센다 — `heuristic` stale 은
    // SIGSTOP·긴 추론·무출력 명령과 관측상 구별되지 않아, 그것까지 "respawn 후보"
    // 로 부르면 일하는 자식을 재시작하라고 권하게 된다. `docs/dev-guide/
    // api-conventions.md` 가 같은 이유로 `stale` 을 기본 terminal state 집합에서
    // 뺀 것과 동일한 판단이다.
    let stale_indices = indices_with(children, |c| {
        state_of(c) == Some("stale")
            && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
    });

    let threshold = host
        .call(
            "settings.get_plugin_setting",
            json!({ "storage_key": "spawn_child_warn_threshold" }),
        )
        .ok()
        .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_SPAWN_CHILD_WARN_THRESHOLD);

    build_spawn_warning(tr, total, &idle_indices, &stale_indices, threshold)
}

fn state_of(child: &Value) -> Option<&str> {
    child.get("state").and_then(|s| s.as_str())
}

/// 조건에 맞는 child 의 `index` 만 모은다.
fn indices_with(children: &[Value], pred: impl Fn(&Value) -> bool) -> Vec<u64> {
    children
        .iter()
        .filter(|c| pred(c))
        .filter_map(|c| c.get("index").and_then(|i| i.as_u64()))
        .collect()
}

fn join_indices(indices: &[u64]) -> String {
    indices
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// host 호출은 `tr`(순수 조회) 뿐 — 단위 테스트 대상.
///
/// 재사용 후보를 **두 목록으로 나눈다.** 둘 다 respawn 대상이지만 근거가 다르다:
/// `idle` 은 자식이 hook 으로 완료를 직접 보고한 값이고, 확정 `stale` 은 보고가 오지
/// 않은 채 호스트 관측이 "전경이 셸로 돌아왔다" 를 잡아낸 값이다(hook 유실 —
/// ADR-0072 가 겨냥한 시나리오). 후자에 "이미 작업을 끝냈다" 는 문구를 쓰면 자식이
/// 그렇게 보고한 적 없는데 보고한 것처럼 읽히므로 문구를 분리한다.
fn build_spawn_warning(
    tr: &Translator,
    total: usize,
    idle_indices: &[u64],
    stale_indices: &[u64],
    threshold: f64,
) -> Option<String> {
    if (total as f64) <= threshold {
        return None;
    }
    let mut msg = tr
        .t("claude.spawn.warning_threshold")
        .replacen("{}", &total.to_string(), 1)
        .replacen("{}", &threshold.to_string(), 1);
    if !idle_indices.is_empty() {
        msg.push_str(&tr.t_fmt(
            "claude.spawn.warning_idle_children",
            &join_indices(idle_indices),
        ));
    }
    if !stale_indices.is_empty() {
        msg.push_str(&tr.t_fmt(
            "claude.spawn.warning_stale_children",
            &join_indices(stale_indices),
        ));
    }
    Some(msg)
}

/// 자식 surface 의 parent 를 조회 — 호스트 `terminal.parent` 로 위임.
pub(crate) fn handle_parent(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = require_surface_id(params, tr)?;
    host_call(host, "terminal.parent", json!({ "surface": surface }))
}

/// 자식 surface 단건 상태 조회 — 호스트 `terminal.state` 로 위임.
/// `claude` namespace 안에 두는 이유는 완료 판정 전략의 `poll_method` 가 owner
/// namespace 밖을 참조할 수 없어서다(결정 2) — `claude.spawn` 기본 전략이 이
/// 메서드를 poll_method 로 참조한다(매니페스트 `[[contributes.completion_strategy]]`).
pub(crate) fn handle_state(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = require_surface_id(params, tr)?;
    host_call(host, "terminal.state", json!({ "surface": surface }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 crate `lang/` 을 로드한 `Translator` — 하드코딩 영문 assertion 을
    /// lang 파일 드리프트로부터 고정한다(`checklist.rs` 의 SENTINEL 핀 테스트와
    /// 동일 패턴).
    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    #[test]
    fn build_spawn_warning_none_below_threshold() {
        let tr = test_translator();
        assert_eq!(build_spawn_warning(&tr, 3, &[], &[], 6.0), None);
    }

    #[test]
    fn build_spawn_warning_above_threshold_lists_idle_and_mentions_respawn() {
        let tr = test_translator();
        let w = build_spawn_warning(&tr, 7, &[2, 5], &[], 6.0).unwrap();
        assert!(w.contains("respawn"));
        assert!(w.contains('2') && w.contains('5'));
    }

    #[test]
    fn build_spawn_warning_above_threshold_no_idle_has_no_respawn_word() {
        let tr = test_translator();
        let w = build_spawn_warning(&tr, 7, &[], &[], 6.0).unwrap();
        assert!(!w.contains("respawn"));
    }

    #[test]
    fn build_spawn_warning_respects_custom_threshold() {
        let tr = test_translator();
        assert_eq!(build_spawn_warning(&tr, 3, &[], &[], 6.0), None);
        assert!(build_spawn_warning(&tr, 4, &[], &[], 3.0).is_some());
    }

    /// 확정 stale 자식만 있어도 respawn 을 권해야 한다 — hook 유실로 idle 보고가
    /// 영영 오지 않는 자식이 정확히 이 경우다(ADR-0072 가 겨냥한 시나리오).
    #[test]
    fn build_spawn_warning_lists_stale_children_as_respawn_candidates() {
        let tr = test_translator();
        let w = build_spawn_warning(&tr, 7, &[], &[3], 6.0).unwrap();
        assert!(w.contains("respawn"), "{w}");
        assert!(w.contains('3'), "{w}");
    }

    /// stale 문구는 idle 문구와 분리된다 — 보고받지 않은 자식에 "이미 끝냈다" 는
    /// 문구를 쓰면 자식이 그렇게 보고한 것처럼 읽힌다.
    #[test]
    fn build_spawn_warning_separates_stale_wording_from_idle() {
        let tr = test_translator();
        let idle_only = build_spawn_warning(&tr, 7, &[2], &[], 6.0).unwrap();
        let stale_only = build_spawn_warning(&tr, 7, &[], &[3], 6.0).unwrap();
        assert!(idle_only.contains("Idle children"), "{idle_only}");
        assert!(!stale_only.contains("Idle children"), "{stale_only}");
        assert!(
            stale_only.contains("never reported completion"),
            "{stale_only}"
        );

        let both = build_spawn_warning(&tr, 7, &[2], &[3], 6.0).unwrap();
        assert!(both.contains("Idle children"), "{both}");
        assert!(both.contains("never reported completion"), "{both}");
    }

    /// `heuristic` stale 은 제외된다 — SIGSTOP·긴 추론과 구별되지 않아 일하는
    /// 자식을 respawn 하라고 권하게 된다. 확정(`foreground_is_shell`)만 센다.
    /// `confidence` 를 안 싣는 옛 호스트 응답도 같은 이유로 안전하게 빠진다.
    #[test]
    fn spawn_warning_counts_only_confirmed_stale() {
        let confirmed_stale = |c: &Value| {
            state_of(c) == Some("stale")
                && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
        };
        let children = vec![
            json!({ "index": 1, "state": "stale", "confidence": "confirmed" }),
            json!({ "index": 2, "state": "stale", "confidence": "heuristic" }),
            json!({ "index": 3, "state": "stale" }),
            json!({ "index": 4, "state": "active", "confidence": "confirmed" }),
        ];
        assert_eq!(indices_with(&children, confirmed_stale), vec![1]);
    }

    #[test]
    fn build_launch_command_no_task() {
        assert_eq!(build_launch_command(None, None), "claude");
    }

    #[test]
    fn build_launch_command_with_simple_task() {
        assert_eq!(build_launch_command(Some("fix"), None), "claude --task fix");
    }

    #[test]
    fn build_launch_command_with_spaces_gets_escaped() {
        let out = build_launch_command(Some("fix the bug"), None);
        assert!(out.starts_with("claude --task "), "prefix wrong: {out}");
        assert!(out.contains("fix the bug"), "task body missing: {out}");
        assert_ne!(out, "claude --task fix the bug", "must be escaped");
    }

    #[test]
    fn build_launch_command_with_profile_appends_quoted_settings_path() {
        assert_eq!(
            build_launch_command(None, Some("/home/user/profile.json")),
            "claude --settings \"/home/user/profile.json\""
        );
    }

    #[test]
    fn build_launch_command_with_task_and_profile_appends_both() {
        assert_eq!(
            build_launch_command(Some("fix"), Some("/home/user/profile.json")),
            "claude --task fix --settings \"/home/user/profile.json\""
        );
    }

    #[test]
    fn claude_launch_command_with_prompt_no_profile_unchanged() {
        let out = claude_launch_command_with_prompt(1, "TASTY_SURFACE_ID=1 ", "hello", None);
        assert!(
            out.starts_with("TASTY_SURFACE_ID=1 claude \"$(cat '"),
            "got {out}"
        );
        assert!(!out.contains("--settings"), "got {out}");
    }

    #[test]
    fn claude_launch_command_with_prompt_and_profile_prepends_settings() {
        let out = claude_launch_command_with_prompt(
            2,
            "TASTY_SURFACE_ID=2 ",
            "hello",
            Some("/home/user/profile.json"),
        );
        assert!(
            out.starts_with(
                "TASTY_SURFACE_ID=2 claude --settings \"/home/user/profile.json\" \"$(cat '"
            ),
            "got {out}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_prompt_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join("tasty-claude-test-write-prompt-file-perms.txt");
        write_prompt_file(&path, "secret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got mode {mode:o}");
        // 테스트 tempfile 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_prompt_file_narrows_permissions_on_reuse() {
        // 이전 실행이 넓은 권한으로 만든 파일이 같은 경로에 이미 있어도, 다시 쓸 때
        // 0600 으로 좁혀져야 한다(재사용 시 구멍 방지 회귀 가드).
        let path = std::env::temp_dir().join("tasty-claude-test-write-prompt-file-reuse.txt");
        std::fs::write(&path, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        write_prompt_file(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "got mode {mode:o}");
        }
        // 테스트 tempfile 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sweep_stale_prompt_files_removes_files_past_ttl() {
        let dir = std::env::temp_dir().join("tasty-claude-test-sweep-removes-stale");
        std::fs::create_dir_all(&dir).unwrap();
        let stale = dir.join(format!("{PROMPT_FILE_PREFIX}old{PROMPT_FILE_SUFFIX}"));
        std::fs::write(&stale, "x").unwrap();
        let old_mtime =
            std::time::SystemTime::now() - (PROMPT_FILE_TTL + std::time::Duration::from_secs(60));
        std::fs::File::open(&stale)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();

        sweep_stale_prompt_files(&dir);

        assert!(!stale.exists(), "stale prompt file should have been swept");
        // 테스트 tempdir 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_stale_prompt_files_keeps_files_within_ttl() {
        let dir = std::env::temp_dir().join("tasty-claude-test-sweep-keeps-fresh");
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = dir.join(format!("{PROMPT_FILE_PREFIX}new{PROMPT_FILE_SUFFIX}"));
        std::fs::write(&fresh, "x").unwrap();

        sweep_stale_prompt_files(&dir);

        assert!(fresh.exists(), "fresh prompt file should not be swept");
        // 테스트 tempdir 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_stale_prompt_files_ignores_non_matching_names() {
        let dir = std::env::temp_dir().join("tasty-claude-test-sweep-ignores-unrelated");
        std::fs::create_dir_all(&dir).unwrap();
        let unrelated = dir.join("some-other-file.txt");
        std::fs::write(&unrelated, "x").unwrap();
        let old_mtime =
            std::time::SystemTime::now() - (PROMPT_FILE_TTL + std::time::Duration::from_secs(60));
        std::fs::File::open(&unrelated)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();

        sweep_stale_prompt_files(&dir);

        assert!(unrelated.exists(), "non-matching file must not be swept");
        // 테스트 tempdir 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 완료 알림 문구 — "spawn 완료" 오독 방지 ──
    // 원래 이 회귀 가드가 겨냥한 한국어 문구는 `lang/ko.toml` 로 옮겨졌으므로,
    // 그 locale 을 실제로 로드해 동일 속성을 검증한다(하드코딩 복제 대신 lang
    // 파일을 SoT 로 삼는다 — `checklist.rs` SENTINEL 핀 테스트와 동일 이유).
    fn test_translator_ko() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "ko")
    }

    #[test]
    fn notify_done_message_leads_with_work_completion() {
        let tr = test_translator_ko();
        let msg = notify_done_message(&tr, "spawn", 42);
        assert!(
            msg.contains("작업 완료"),
            "완료 대상이 '작업'임이 드러나야 함: {msg}"
        );
        assert!(msg.contains("42"), "target surface 번호 누락: {msg}");
        assert!(msg.contains("spawn"), "호출 방식 정보 누락: {msg}");
    }

    #[test]
    fn notify_done_message_does_not_read_as_command_itself_completing() {
        // 회귀 방지: 과거 "{command_name} 완료: surface N" 형태는 "spawn 이라는 동작이
        // 완료됐다"로 오독되기 쉬웠다 — command_name 이 더 이상 완료의 주어로 문장
        // 맨 앞에 오지 않아야 한다.
        let tr = test_translator_ko();
        for command_name in ["spawn", "tell"] {
            let msg = notify_done_message(&tr, command_name, 7);
            assert!(
                !msg.starts_with(&format!("{command_name} 완료")),
                "옛 오독 유발 포맷으로 회귀함: {msg}"
            );
        }
    }

    #[test]
    fn require_child_index_missing_is_invalid_params() {
        let tr = test_translator();
        let err = require_child_index(&json!({ "surface_id": 1 }), &tr).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    // ── 형제 once-hook 정리 재현 (docs/plugins/claude/index.md 의 notify-done 형제 hook 정리 절 참조) ──

    use std::cell::RefCell;

    struct MockHook {
        id: u64,
        surface_id: u32,
        command: String,
        event: String,
        /// once=false 인 상시 hook 은 발화해도 남는다(에러 정지 알림 hook).
        once: bool,
    }

    /// hook.set/list/unset + terminal.tell 을 in-memory 로 시뮬레이션하는 mock 호스트.
    /// `fire` 로 특정 surface 의 특정 event once-hook 을 실제 host 처럼 제거(once)한다.
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

        /// event 발화 시뮬레이션 — 매칭 once-hook 제거(호스트 `check_and_fire` 의
        /// retain 과 동일). 상시 hook(once=false)은 남긴다. 발화한 hook 개수를 반환.
        fn fire(&self, surface_id: u32, event: &str) -> usize {
            let mut hooks = self.hooks.borrow_mut();
            let fired = hooks
                .iter()
                .filter(|h| h.surface_id == surface_id && h.event == event)
                .count();
            hooks.retain(|h| !(h.surface_id == surface_id && h.event == event && h.once));
            fired
        }

        /// 완료 알림(notify-done) 그룹의 command 만 — 에러 정지 알림 hook 은 별개
        /// 수명이라 형제 사이클 assertion 에서 제외한다.
        fn done_commands_on(&self, surface_id: u32) -> Vec<String> {
            self.commands_on(surface_id)
                .into_iter()
                .filter(|c| c.contains("notify-done"))
                .collect()
        }

        /// 특정 event 로 등록된 hook 개수.
        fn hooks_for_event(&self, surface_id: u32, event: &str) -> usize {
            self.hooks
                .borrow()
                .iter()
                .filter(|h| h.surface_id == surface_id && h.event == event)
                .count()
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
                        once: params["once"].as_bool().unwrap_or(false),
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
        // 같은 target surface 에 spawn 그룹과 tell 그룹이 공존(concurrent 등록).
        let spawn_cmd = notify_done_command(9, 100, "spawn");
        let tell_cmd = notify_done_command(9, 100, "tell");
        let hooks = vec![
            json!({ "id": 1, "command": spawn_cmd, "event": "process-exit" }),
            json!({ "id": 2, "command": tell_cmd, "event": "process-exit" }),
            json!({ "id": 3, "command": spawn_cmd, "event": "claude-idle" }),
        ];
        // spawn command 로 정리 → spawn 그룹(id 1,3)만, tell 그룹(id 2)은 제외.
        let ids = siblings_to_unset(&hooks, &spawn_cmd);
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn sibling_cleanup_removes_all_after_one_fires() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(host.done_commands_on(target).len(), 3, "3 형제 등록");

        // needs-input 이 fire(once 제거) → 나머지 형제(claude-idle, process-exit) 정리.
        assert_eq!(host.fire(target, "needs-input"), 1);
        let expected = notify_done_command(caller, target, "tell");
        cleanup_sibling_hooks(&host, target, &expected);

        assert!(
            host.done_commands_on(target).is_empty(),
            "형제 hook 이 하나도 남지 않아야 함 — process-exit 좀비 없음: {:?}",
            host.done_commands_on(target)
        );
    }

    #[test]
    fn concurrent_registrations_leave_no_zombie() {
        // 같은 child(target) 에 spawn 완료 hook 과 tell 완료 hook 이 겹쳐 등록된 상태
        // (spawn 후 fire 전에 tell 이 들어온 경우). 단일 meta 슬롯을 덮어쓰던 옛
        // 방식이면 여기서 spawn 그룹의 process-exit 이 좀비로 남았다.
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "spawn");
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(host.done_commands_on(target).len(), 6, "두 그룹 = 6 hook");
        // 에러 정지 hook 은 command 에 command_name 이 없어 두 그룹이 같은 문자열을
        // 쓴다 — 멱등 등록이라 두 번 불러도 1 개만 남는다.
        assert_eq!(
            host.hooks_for_event(target, crate::error_scan::STALLED_EVENT),
            1,
            "에러 정지 hook 은 중복 등록되지 않아야 함"
        );

        // spawn 그룹의 claude-idle 이 먼저 fire → spawn 그룹만 정리.
        host.fire(target, "claude-idle");
        let spawn_cmd = notify_done_command(caller, target, "spawn");
        cleanup_sibling_hooks(&host, target, &spawn_cmd);

        let remaining = host.done_commands_on(target);
        let tell_cmd = notify_done_command(caller, target, "tell");
        // tell 그룹은 그대로(claude-idle fire 가 tell 의 claude-idle 도 제거했으므로 2개),
        // spawn 그룹은 완전히 사라져야 한다.
        assert!(
            remaining.iter().all(|c| c == &tell_cmd),
            "spawn 그룹 좀비 잔존: {remaining:?}"
        );
        assert!(
            !remaining.iter().any(|c| c == &spawn_cmd),
            "spawn 그룹 process-exit 좀비 남음"
        );

        // 이제 tell 그룹도 fire → 전부 정리.
        host.fire(target, "needs-input");
        cleanup_sibling_hooks(&host, target, &tell_cmd);
        assert!(
            host.done_commands_on(target).is_empty(),
            "최종적으로 형제 hook 이 전부 사라져야 함: {:?}",
            host.done_commands_on(target)
        );
    }

    // ── 자기재무장(self-rearm) — child 가 살아있는 동안 알림 반복 (docs/plugins/claude/index.md 의 자기재무장 절 참조) ──
    //
    // 배경: needs-input/claude-idle 은 process-exit 와 달리 "child 가 아직 살아있는
    // 상태 전환"일 수 있다(예: 애매한 지시에 되묻고 다시 작업 재개). 형제 hook 이
    // once=true 라 한 번 fire 하면 남은 형제도 정리돼 그 spawn/tell 콜당 알림이 딱
    // 1번만 오던 문제 — child 가 진짜 완료되기 전에 needs-input 을 한 번이라도 거치면
    // 그 뒤엔 재알림 경로가 없었다.

    #[test]
    fn handle_notify_done_rearms_when_target_still_alive() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(host.done_commands_on(target).len(), 3, "최초 3 형제 등록");

        let tr = test_translator();

        // 1번째 전환: needs-input(되묻기) — child 는 여전히 살아있다.
        assert_eq!(host.fire(target, "needs-input"), 1);
        handle_notify_done(
            &host,
            &json!({ "caller_surface": caller, "target_surface": target, "command": "tell" }),
            &tr,
        )
        .unwrap();
        assert_eq!(
            host.done_commands_on(target).len(),
            3,
            "살아있으면 형제 hook 이 다시 3개로 재무장돼야 함"
        );

        // 2번째 전환: 진짜 완료(claude-idle) — 여전히 살아있는 상태에서 fire 됐다고
        // 가정(실제로는 이 직후 host 가 종료를 감지해도, hook 발화 자체는 idle 이 먼저
        // 다다르는 케이스를 재현). 재무장이 반복되는지 확인.
        assert_eq!(host.fire(target, "claude-idle"), 1);
        handle_notify_done(
            &host,
            &json!({ "caller_surface": caller, "target_surface": target, "command": "tell" }),
            &tr,
        )
        .unwrap();
        assert_eq!(
            host.done_commands_on(target).len(),
            3,
            "두 번째 전환에도 계속 재무장돼야 함 — 'spawn/tell 당 1회' 로 되돌아가면 안 됨"
        );
    }

    // ── 에러 정지 알림 배선 (`claude-error-stalled`) ──

    #[test]
    fn spawn_tell_wiring_subscribes_the_error_axis() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "spawn");
        assert_eq!(
            host.hooks_for_event(target, crate::error_scan::STALLED_EVENT),
            1,
            "완료 3종과 함께 에러 정지 hook 도 등록돼야 함"
        );
        assert!(
            host.commands_on(target)
                .iter()
                .any(|c| c == &notify_error_command(caller, target))
        );
    }

    #[test]
    fn error_hook_survives_the_sibling_fire_cleanup_rearm_cycle() {
        // 형제 그룹은 fire → 정리 → 재무장 사이클을 도는데, 에러 정지 hook 은 그
        // 사이클에 휘말리지 않아야 한다(수명이 다르다).
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, caller, target, "tell");
        let tr = test_translator();

        for _ in 0..3 {
            assert_eq!(host.fire(target, "needs-input"), 1);
            handle_notify_done(
                &host,
                &json!({ "caller_surface": caller, "target_surface": target, "command": "tell" }),
                &tr,
            )
            .unwrap();
            assert_eq!(
                host.hooks_for_event(target, crate::error_scan::STALLED_EVENT),
                1,
                "재무장 사이클을 돌아도 에러 hook 은 정확히 1개로 유지"
            );
            assert_eq!(host.done_commands_on(target).len(), 3, "형제 재무장도 정상");
        }
    }

    #[test]
    fn error_hook_is_standing_so_repeated_stalls_keep_notifying() {
        // once=true 였다면 첫 발화 후 사라져 두 번째 정지를 놓친다.
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "spawn");
        assert_eq!(host.fire(target, crate::error_scan::STALLED_EVENT), 1);
        assert_eq!(
            host.hooks_for_event(target, crate::error_scan::STALLED_EVENT),
            1,
            "상시 hook 이라 발화해도 남아야 한다"
        );
        assert_eq!(host.fire(target, crate::error_scan::STALLED_EVENT), 1);
    }

    #[test]
    fn handle_notify_error_requires_both_surfaces() {
        let host = MockHost::new();
        let tr = test_translator();
        assert_eq!(
            handle_notify_error(&host, &json!({ "target_surface": 5 }), &tr)
                .unwrap_err()
                .code,
            -32602
        );
        assert_eq!(
            handle_notify_error(&host, &json!({ "caller_surface": 5 }), &tr)
                .unwrap_err()
                .code,
            -32602
        );
    }

    #[test]
    fn notify_error_message_appends_error_line_hint() {
        // codex `notify-caller` 선례 — 알림 조립 직전 화면을 읽어 실패 원인을 덧붙인다.
        struct ScreenHost(&'static str);
        impl HostCall for ScreenHost {
            fn call(
                &self,
                method: &str,
                _params: Value,
            ) -> Result<Value, tasty_plugin_sdk::PluginError> {
                match method {
                    "surface.screen_text" => Ok(json!({ "text": self.0 })),
                    other => panic!("unexpected host call: {other}"),
                }
            }
        }
        let tr = test_translator();
        let with_hint = notify_error_message(
            &tr,
            &ScreenHost("working…\n  API Error: Connection error\n"),
            42,
        );
        assert!(with_hint.contains("42"), "대상 surface 가 문구에 있어야 함");
        assert!(
            with_hint.contains("API Error: Connection error"),
            "에러 줄 힌트: {with_hint}"
        );

        // 화면에 에러 줄이 없으면 힌트 없이 본문만.
        let plain = notify_error_message(&tr, &ScreenHost("all good\n"), 42);
        assert!(!plain.contains("Last error"), "{plain}");
    }

    #[test]
    fn handle_notify_done_does_not_rearm_when_target_exited() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, caller, target, "spawn");

        // process-exit 로 fire — host 는 이 시점에 이미 동기로 surface 를 닫으므로
        // surface.locate 가 exists:false 를 돌려주는 상황을 재현.
        assert_eq!(host.fire(target, "process-exit"), 1);
        host.mark_dead(target);
        let tr = test_translator();
        handle_notify_done(
            &host,
            &json!({ "caller_surface": caller, "target_surface": target, "command": "spawn" }),
            &tr,
        )
        .unwrap();

        assert!(
            host.done_commands_on(target).is_empty(),
            "죽은 surface 에 재무장하면 좀비 hook: {:?}",
            host.done_commands_on(target)
        );
    }
}
