//! codex.* 요청을 처리한다. 자식 관리는 호스트의 terminal.* IPC에 맡긴다.
//! 여기서는 기동 명령을 만들고 Codex 훅을 상태·알림 호출로 변환한다.
//! 모든 host.call은 응답을 동기로 기다린다.

use serde_json::{Map, Value, json};
use tasty_plugin_agent_common::children::{join_indices, spawn_census};
use tasty_plugin_agent_common::host_call::{HostCall, cleanup_sibling_hooks, surface_is_alive};
use tasty_plugin_agent_common::params::{
    TargetSurfaceError, U32FieldError, forward, target_surface,
};
use tasty_plugin_agent_common::prompt_file;
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

/// 번역 문구의 여러 자리표시자를 채운다.
pub(crate) fn t_args(tr: &Translator, key: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = tr.t(key).to_string();
    for (token, value) in pairs {
        out = out.replace(token, value);
    }
    out
}

/// 호스트 오류를 IPC 오류로 변환한다. 이미 포함된 오류 접두어를 다시 붙이지 않는다.
fn host_call<H: HostCall>(host: &H, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    host.call(method, params).map_err(IpcMethodError::from)
}

/// 필수 u32 값을 읽고 공용 검사 결과를 이 플러그인의 오류 문구로 바꾼다.
/// 누락과 잘못된 값을 구분하며, 범위를 넘는 ID를 자르지 않고 거부한다.
pub(crate) fn require_u32(
    params: &Value,
    key: &str,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    tasty_plugin_agent_common::params::u32_field(params, key).map_err(|e| match e {
        U32FieldError::Missing => {
            IpcMethodError::invalid_params(&tr.t_replace("codex.params.missing", "{key}", key))
        }
        U32FieldError::Malformed { raw } => IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.not_a_number",
            &[("{key}", key), ("{raw}", &raw)],
        )),
    })
}

/// surface와 surface_id를 공용 함수로 검사하고 오류 문구를 번역한다.
pub(crate) fn optional_target_surface(
    params: &Value,
    tr: &Translator,
) -> Result<Option<u32>, IpcMethodError> {
    target_surface(params).map_err(|e| match e {
        TargetSurfaceError::Malformed { key, raw } => IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.not_a_number",
            &[("{key}", key), ("{raw}", &raw)],
        )),
        TargetSurfaceError::Conflict {
            surface,
            surface_id,
        } => IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.surface_conflict",
            &[
                ("{surface}", &surface.to_string()),
                ("{surface_id}", &surface_id.to_string()),
            ],
        )),
    })
}

pub(crate) fn require_target_surface(
    params: &Value,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    optional_target_surface(params, tr)?.ok_or_else(|| {
        // 두 필드 모두 사용할 수 있으므로 전용 안내를 반환한다.
        IpcMethodError::invalid_params(tr.t("codex.params.missing_target_surface"))
    })
}

/// 지정된 대상만 호스트에 전달한다. 생략하면 호스트의 단일 부모 선택 규칙을 따른다.
fn put_target_surface(
    dst: &mut serde_json::Map<String, Value>,
    params: &Value,
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    if let Some(surface) = optional_target_surface(params, tr)? {
        dst.insert("surface".into(), json!(surface));
    }
    Ok(())
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// Claude 프롬프트 파일과 구분해 정리할 수 있도록 별도 접두어를 사용한다.
const PROMPT_FILE_PREFIX: &str = "tasty-codex-prompt-";
/// POSIX 셸에 보낼 Codex 명령을 만든다. 프롬프트는 파일로 전달해 셸 이력 확장을 피한다.
/// 파일 쓰기에 실패해도 경고 후 명령을 만든다. 이전 파일 정리는 공용 TTL 규칙을 따른다.
/// TASTY_SURFACE_ID를 지정하고 훅 신뢰 우회 플래그로 훅 실행을 요청한다.
/// 이 플래그가 모든 상황에서 훅 전달을 보장하지는 않는다.
fn make_codex_command(surface_id: u32, prompt: Option<&str>, policy_args: &str) -> String {
    let prefix = format!(
        "TASTY_SURFACE_ID={surface_id} {} ",
        crate::POSIX_CODEX_COMMAND
    );
    let policy_suffix = if policy_args.is_empty() {
        String::new()
    } else {
        format!(" {policy_args}")
    };
    match prompt {
        Some(p) if !p.is_empty() => {
            let temp_dir = std::env::temp_dir();
            prompt_file::sweep_stale(&temp_dir, PROMPT_FILE_PREFIX);
            let prompt_path = prompt_file::path_for(&temp_dir, PROMPT_FILE_PREFIX, surface_id);
            if let Err(e) = prompt_file::write(&prompt_path, p) {
                tracing::warn!("Failed to write codex prompt file: {e}");
            }
            format!(
                "{prefix}--dangerously-bypass-hook-trust{policy_suffix} \"$(cat '{}')\"\r",
                prompt_path.display()
            )
        }
        _ => format!("{prefix}--dangerously-bypass-hook-trust{policy_suffix}\r"),
    }
}

const VALID_APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];
const VALID_SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];

fn validate_choice(
    flag_name: &str,
    value: &str,
    valid: &[&str],
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    if valid.contains(&value) {
        Ok(())
    } else {
        Err(IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.invalid_choice",
            &[
                ("{flag}", flag_name),
                ("{value}", value),
                ("{valid}", &valid.join(", ")),
            ],
        )))
    }
}

/// 전역 설정(`default_approval_policy`/`default_sandbox_mode`)에서 fallback 값을
/// 읽는다. 미설정이거나 `"inherit"`이면 None.
fn global_policy_default<H: HostCall>(host: &H, storage_key: &str) -> Option<String> {
    host.call(
        "settings.get_plugin_setting",
        json!({ "storage_key": storage_key }),
    )
    .ok()
    .and_then(|v| v.get("value").and_then(|v| v.as_str()).map(String::from))
    .filter(|s| s != "inherit" && !s.is_empty())
}

/// 명시한 승인·샌드박스 정책을 설정값보다 우선한다.
/// 둘 다 없으면 승인 정책은 never, 샌드박스는 Codex 기본값을 사용한다.
/// full_auto는 두 제한을 우회하는 플래그로 바꾸며 명시한 개별 정책과의 병용은 거부한다.
pub(crate) fn resolve_policy_args<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    let full_auto = params
        .get("full_auto")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let approval = optional_str(params, "approval");
    let sandbox = optional_str(params, "sandbox");

    if full_auto {
        if approval.is_some() || sandbox.is_some() {
            return Err(IpcMethodError::invalid_params(
                tr.t("codex.params.full_auto_conflict"),
            ));
        }
        return Ok("--dangerously-bypass-approvals-and-sandbox".to_string());
    }

    let approval = match approval {
        Some(v) => {
            validate_choice("approval", &v, VALID_APPROVAL_POLICIES, tr)?;
            Some(v)
        }
        None => Some(
            global_policy_default(host, "default_approval_policy")
                .unwrap_or_else(|| "never".to_string()),
        ),
    };
    let sandbox = match sandbox {
        Some(v) => {
            validate_choice("sandbox", &v, VALID_SANDBOX_MODES, tr)?;
            Some(v)
        }
        None => global_policy_default(host, "default_sandbox_mode"),
    };

    let mut parts = Vec::new();
    if let Some(a) = approval {
        parts.push(format!("-a {a}"));
    }
    if let Some(s) = sandbox {
        parts.push(format!("-s {s}"));
    }
    Ok(parts.join(" "))
}

pub(crate) fn handle_launch(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("codex")
        .to_string();
    let directory = optional_str(params, "directory");
    let task = optional_str(params, "task");

    // cwd는 PTY의 시작 경로로 전달하므로 셸에 cd를 보내지 않는다.
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
            IpcMethodError::new(tr.t_replace(
                "codex.launch.workspace_create_missing_id",
                "{resp}",
                &ws_result.to_string(),
            ))
        })? as u32;
    let surface_id = ws_result
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if let Some(sid) = surface_id {
        let policy_args = resolve_policy_args(host, params, tr)?;
        let cmd = make_codex_command(sid, task.as_deref(), &policy_args);
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

pub(crate) fn handle_parent(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = require_target_surface(params, tr)?;
    host_call(host, "terminal.parent", json!({ "surface": surface }))
}

/// 완료 전략의 poll_method가 같은 namespace를 사용하도록 terminal.state를 중계한다.
pub(crate) fn handle_state(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = require_target_surface(params, tr)?;
    host_call(host, "terminal.state", json!({ "surface": surface }))
}

pub(crate) fn handle_tell(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_target_surface(params, tr)?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_message")))?;
    // 개행과 제출 처리는 호스트의 terminal.tell에 맡긴다.
    let resp = host_call(
        host,
        "terminal.tell",
        json!({ "surface": surface_id, "text": message }),
    )?;

    // CLI는 호출자 ID를 자동으로 채운다. 유효한 값이 없으면 알림을 등록하지 않는다.
    if let Ok(caller) = require_u32(params, "caller_surface", tr) {
        register_notify_hooks(host, caller, surface_id, "tell");
    }

    Ok(resp)
}

pub(crate) fn handle_spawn(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let parent_surface = require_target_surface(params, tr)?;
    let prompt = optional_str(params, "prompt");

    // 먼저 자식 터미널을 만들고 아래에서 Codex 기동 명령을 보낸다.
    let mut sp = forward(params, &["workspace", "pane", "cwd", "role", "nickname"]);
    sp.insert("parent".into(), json!(parent_surface));
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.spawn.missing_child_surface_id",
                "{resp}",
                &resp.to_string(),
            ))
        })?;

    let policy_args = resolve_policy_args(host, params, tr)?;
    let cmd = make_codex_command(child_sid, prompt.as_deref(), &policy_args);
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    register_notify_hooks(host, parent_surface, child_sid, "spawn");

    // 경고 기준을 넘어도 spawn은 성공으로 반환한다.
    let mut out = resp;
    if let Some(warning) = compute_spawn_warning(host, parent_surface, tr) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("warning".into(), json!(warning));
        }
    }

    Ok(out)
}

/// 등록과 정리에 같은 명령을 사용해 대상별 완료 훅 그룹을 찾는다.
fn notify_caller_command(caller_surface: u32, target_surface: u32, kind: &str) -> String {
    format!(
        "tasty codex notify-caller --caller {caller_surface} --target {target_surface} --kind {kind}"
    )
}

/// 자식이 맡은 작업의 완료를 알린다. spawn/tell은 호출 방식으로 따로 표시한다.
fn notify_caller_message(tr: &Translator, kind: &str, target: u32) -> String {
    tr.t("codex.notify.done_message")
        .replace("{target}", &target.to_string())
        .replace("{kind}", kind)
}

/// 화면에서 샌드박스 초기화 실패를 추정할 때 찾는 문구. 대화 내용에도 나타날 수 있다.
const SANDBOX_FAILURE_MARKER: &str = "RTM_NEWADDR";

/// 완료 로그에 덧붙일 번역된 샌드박스 안내의 키.
const SANDBOX_FAILURE_HINT_KEY: &str = "codex.notify.sandbox_hint";

/// 화면에 오류 표식이 있으면 샌드박스 초기화 실패 가능성을 안내한다.
fn detect_sandbox_failure_hint(tr: &Translator, screen_text: &str) -> Option<String> {
    if screen_text.contains(SANDBOX_FAILURE_MARKER) {
        Some(tr.t(SANDBOX_FAILURE_HINT_KEY).to_string())
    } else {
        None
    }
}

/// 화면 조회에 실패하거나 표식이 없으면 원래 완료 문구만 반환한다.
fn append_sandbox_hint_if_detected(
    tr: &Translator,
    base: String,
    screen_text: Option<&str>,
) -> String {
    match screen_text.and_then(|s| detect_sandbox_failure_hint(tr, s)) {
        Some(hint) => format!("{base} {hint}"),
        None => base,
    }
}

/// 오류 표식을 찾을 최근 화면 줄 수.
const SCREEN_TEXT_SCAN_LINES: u64 = 800;

/// 힌트에 쓸 화면 내용을 읽는다. 조회에 실패하면 힌트만 생략한다.
fn fetch_screen_text_for_hint<H: HostCall>(host: &H, target: u32) -> Option<String> {
    host.call(
        "surface.screen_text",
        json!({ "surface_id": target, "lines": SCREEN_TEXT_SCAN_LINES }),
    )
    .ok()
    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_string))
}

/// codex-idle·needs-input·process-exit 완료 훅을 등록한다.
/// 등록 실패는 경고만 남기며 spawn/tell 성공을 취소하지 않는다.
fn register_notify_hooks<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
    kind: &str,
) {
    let cmd = notify_caller_command(caller_surface, target_surface, kind);
    // 세 이벤트는 플러그인 매니페스트에도 선언돼 있어야 한다.
    tasty_plugin_agent_common::host_call::register_completion_hooks(
        host,
        target_surface,
        &cmd,
        &["codex-idle", "needs-input", "process-exit"],
        "codex",
    );
}

/// 완료 로그를 남기고 같은 대상·명령의 훅을 정리한다. 대상이 조회되면 다시 등록한다.
pub(crate) fn handle_notify_caller<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let caller = require_u32(params, "caller", tr)?;
    let target = require_u32(params, "target", tr)?;
    let kind = optional_str(params, "kind").unwrap_or_else(|| "tell".into());
    let message = notify_caller_message(tr, &kind, target);
    let screen_text = fetch_screen_text_for_hint(host, target);
    let message = append_sandbox_hint_if_detected(tr, message, screen_text.as_deref());

    // 로그 쓰기에 실패해도 경고를 남기고 훅 정리를 계속한다.
    if let Err(e) = tasty_utils::notify::append_notify_line(caller, &message) {
        tracing::warn!("codex notify-caller completion-log append failed: {e}");
    }

    let expected_command = notify_caller_command(caller, target, &kind);
    cleanup_sibling_hooks(host, target, &expected_command);

    rearm_if_still_alive(host, caller, target, &kind);

    Ok(json!({}))
}

/// 대상 터미널이 조회되면 완료 훅을 다시 등록한다. 조회 실패 시 생략한다.
/// 터미널의 존재 여부만 확인하므로 프로세스가 실행 중이라는 뜻은 아니다.
fn rearm_if_still_alive<H: HostCall>(host: &H, caller: u32, target: u32, kind: &str) {
    if surface_is_alive(host, target) {
        register_notify_hooks(host, caller, target, kind);
    }
}

/// 자식 목록과 상태를 읽어 경고를 만든다. 조회 실패 시 경고를 생략한다.
fn compute_spawn_warning(
    host: &HostHandle,
    parent_surface_id: u32,
    tr: &Translator,
) -> Option<String> {
    let c = spawn_census(host, parent_surface_id)?;
    build_spawn_warning(tr, c.total, &c.idle, &c.stale, c.threshold)
}

/// 자식 수가 기준을 넘으면 경고한다. idle과 stale은 확인 근거가 달라 따로 안내한다.
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
        .t_replace("codex.spawn_warning.total", "{total}", &total.to_string())
        .replace("{threshold}", &threshold.to_string());
    if !idle_indices.is_empty() {
        msg.push_str(&tr.t_replace(
            "codex.spawn_warning.idle",
            "{indices}",
            &join_indices(idle_indices),
        ));
    }
    if !stale_indices.is_empty() {
        msg.push_str(&tr.t_replace(
            "codex.spawn_warning.stale",
            "{indices}",
            &join_indices(stale_indices),
        ));
    }
    Some(msg)
}

/// 호스트의 children 응답을 그대로 반환한다. Claude 플러그인의 배열 응답과 다르다.
pub(crate) fn handle_children<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let mut cp = serde_json::Map::new();
    put_target_surface(&mut cp, params, tr)?;
    host_call(host, "terminal.children", Value::Object(cp))
}

/// 부모와 선택한 역할의 자식에게 보낼 텍스트를 호스트에 전달한다.
pub(crate) fn handle_broadcast(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_text")))?;
    let mut bp = forward(params, &["role"]);
    put_target_surface(&mut bp, params, tr)?;
    bp.insert("text".into(), json!(text));
    host_call(host, "terminal.broadcast", Value::Object(bp))
}

/// 호스트의 killed_surface_id·child_index 응답을 그대로 반환한다.
pub(crate) fn handle_kill<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let child = require_u32(params, "child", tr)?;
    let mut kp = serde_json::Map::new();
    put_target_surface(&mut kp, params, tr)?;
    kp.insert("child".into(), json!(child));
    host_call(host, "terminal.kill", Value::Object(kp))
}

pub(crate) fn handle_respawn(
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let child = require_u32(params, "child", tr)?;
    let prompt = optional_str(params, "prompt");

    // 자식 재시작과 상태 갱신은 호스트에 맡긴 뒤 Codex 명령을 보낸다.
    let mut rp = forward(params, &["cwd", "role", "nickname"]);
    put_target_surface(&mut rp, params, tr)?;
    rp.insert("child".into(), json!(child));
    let resp = host_call(host, "terminal.respawn", Value::Object(rp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.respawn.missing_child_surface_id",
                "{resp}",
                &resp.to_string(),
            ))
        })?;

    let policy_args = resolve_policy_args(host, params, tr)?;
    let cmd = make_codex_command(child_sid, prompt.as_deref(), &policy_args);
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    Ok(resp)
}

/// 설치한 Codex 훅을 호스트 상태와 알림으로 변환한다.
/// 전파하지 않은 호출 실패는 host_call_failures에 센다.
/// Codex용 셸 명령은 이 내부 응답을 버리고 빈 객체를 반환한다.
pub(crate) fn handle_hook<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_event")))?;
    // 대상 누락에만 훅 전용 안내를 쓰고 형식 오류·값 충돌은 그대로 반환한다.
    let surface_id = optional_target_surface(params, tr)?
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.hook.requires_surface")))?;
    if event == "session-end" {
        return handle_session_end(host, surface_id);
    }
    let new_state = hook_event_to_state(event, tr)?;

    // terminal.set_state 실패는 반환 오류로 전파하므로 이 수에는 포함하지 않는다.
    let mut host_call_failures: usize = 0;
    // 재시작·복원에 쓸 세션 메타데이터를 기록한다.
    if event == "session-start"
        && let Some(session) = params.get("session").and_then(|v| v.as_str())
        && !session.is_empty()
    {
        host_call_failures += record_session_meta(host, surface_id, session);
    }
    // 상태 갱신 실패는 호출자에게 알린다. 뒤의 알림 실패는 집계하고 계속한다.
    host_call(
        host,
        "terminal.set_state",
        json!({"surface":surface_id,"state":new_state}),
    )?;
    // 턴 종료 훅과 승인 대기 화면 알림은 상태 갱신과 별도로 보낸다.
    host_call_failures += apply_hook_side_effects(host, event, surface_id);
    Ok(json!({ "host_call_failures": host_call_failures }))
}

/// session-end에서는 세션 메타데이터만 지운다. 삭제 실패는 집계해 반환한다.
fn handle_session_end<H: HostCall>(host: &H, surface_id: u32) -> Result<Value, IpcMethodError> {
    let mut failures = 0;
    if let Err(error) = host.call(
        "surface.meta.unset",
        json!({"surface_id":surface_id,"key":"codex-session-id"}),
    ) {
        tracing::warn!("codex session-end metadata cleanup: {error}");
        failures += 1;
    }
    Ok(json!({"host_call_failures":failures}))
}

/// 세션 ID와 복원 명령을 기록하고 실패한 호출 수를 반환한다.
fn record_session_meta<H: HostCall>(host: &H, surface_id: u32, session: &str) -> usize {
    let mut failures = 0;
    for (key, value) in [
        ("codex-session-id", session.to_string()),
        ("restore.command", format!("codex resume {session}")),
    ] {
        if let Err(e) = host.call(
            "surface.meta.set",
            json!({ "surface_id": surface_id, "key": key, "value": value }),
        ) {
            tracing::warn!("codex hook meta.set '{key}' failed: {e}");
            failures += 1;
        }
    }
    failures
}

/// 턴 종료·승인 대기 알림을 보내고 실패한 호출 수를 반환한다.
fn apply_hook_side_effects<H: HostCall>(host: &H, event: &str, surface_id: u32) -> usize {
    let mut failures = 0;
    for (event_key, value) in hook_side_effects(event) {
        if let Err(e) = host.call(event_key, value(surface_id)) {
            tracing::warn!("codex hook '{event}' side-effect {event_key} failed: {e}");
            failures += 1;
        }
    }
    failures
}

/// Codex 훅을 상태로 변환한다. interrupt는 idle로 바꿔 중단된 턴을 알린다.
/// post-tool-use는 도구 실행 후 active로 바꾸므로 승인 직후 needs_input을 해제하지는 못한다.
/// 화면 알림의 확인·삭제는 이 상태 변경과 별개다.
fn hook_event_to_state(event: &str, tr: &Translator) -> Result<&'static str, IpcMethodError> {
    match event {
        "stop" | "interrupt" => Ok("idle"),
        "session-end" => Ok("exited"),
        "permission-request" => Ok("needs_input"),
        "prompt-submit" | "session-start" | "post-tool-use" => Ok("active"),
        other => Err(IpcMethodError::invalid_params(&tr.t_replace(
            "codex.hook.unknown_event",
            "{event}",
            other,
        ))),
    }
}

/// 상태 변경 외에 보낼 알림 호출을 만든다.
/// stop/interrupt는 완료 훅을, permission-request는 입력 대기 훅과 화면 알림을 보낸다.
fn hook_side_effects(event: &str) -> Vec<(&'static str, fn(u32) -> Value)> {
    match event {
        "stop" | "interrupt" => vec![(
            "surface.fire_hook",
            |sid| json!({ "surface_id": sid, "event": "codex-idle" }),
        )],
        "permission-request" => vec![
            (
                "surface.fire_hook",
                |sid| json!({ "surface_id": sid, "event": "needs-input" }),
            ),
            (
                "surface.completion",
                |sid| json!({ "surface_id": sid, "kind": "needs_input" }),
            ),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
use crate::install::*;

#[cfg(test)]
// 시험의 let _는 제품 코드에서 반환값을 버리는 목록에 포함하지 않는다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// 번역 카탈로그를 사용해 누락과 형식 오류 문구를 구분한다.
    fn absent_msg(tr: &Translator, key: &str) -> String {
        tr.t_replace("codex.params.missing", "{key}", key)
    }

    fn malformed_msg(tr: &Translator, key: &str, raw: &str) -> String {
        t_args(
            tr,
            "codex.params.not_a_number",
            &[("{key}", key), ("{raw}", raw)],
        )
    }

    #[test]
    fn require_u32_separates_absent_from_malformed_and_refuses_to_truncate() {
        let tr = test_translator();
        let e = require_u32(&json!({}), "surface", &tr).unwrap_err();
        assert!(e.message.contains(&absent_msg(&tr, "surface")), "{e:?}");

        assert_eq!(
            require_u32(&json!({ "surface": 0 }), "surface", &tr).unwrap(),
            0
        );
        assert_eq!(
            require_u32(&json!({ "surface": u32::MAX }), "surface", &tr).unwrap(),
            u32::MAX
        );

        let e = require_u32(&json!({ "surface": "conductor" }), "surface", &tr).unwrap_err();
        let m = e.message.clone();
        assert!(
            m.contains(&malformed_msg(&tr, "surface", "\"conductor\"")),
            "{m}"
        );
        assert!(
            !m.contains(&absent_msg(&tr, "surface")),
            "값이 왔는데 없다고 답한다: {m}"
        );

        // 범위를 넘는 ID를 자르면 다른 대상을 가리키므로 거부해야 한다.
        for over in [
            u64::from(u32::MAX) + 1,
            u64::from(u32::MAX) + 2,
            5_000_000_000,
        ] {
            let e = require_u32(&json!({ "surface": over }), "surface", &tr).unwrap_err();
            assert!(
                e.message
                    .contains(&malformed_msg(&tr, "surface", &over.to_string())),
                "범위를 넘는 {over}을 거부해야 한다"
            );
        }

        assert!(require_u32(&json!({ "surface": -1 }), "surface", &tr).is_err());
    }

    /// 모든 언어에서 누락과 형식 오류를 구분해야 한다.
    #[test]
    fn the_two_branches_stay_distinguishable_in_every_locale() {
        for locale in ["en", "ko", "ja"] {
            let tr = test_translator_for(locale);
            let absent = absent_msg(&tr, "surface");
            let malformed = malformed_msg(&tr, "surface", "5000000000");
            assert_ne!(
                absent, malformed,
                "{locale}: 누락과 형식 오류의 문구가 같다"
            );
            assert!(
                !absent.contains("{key}"),
                "{locale}: 자리표시자가 치환되지 않았다"
            );
            assert!(
                !malformed.contains("{key}") && !malformed.contains("{raw}"),
                "{locale}: 자리표시자가 치환되지 않았다 — {malformed}"
            );
        }
    }

    /// 모든 언어에서 잘못된 필드 이름을 오류에 포함해야 한다.
    #[test]
    fn a_malformed_target_surface_names_which_of_the_two_keys_was_wrong() {
        for locale in ["en", "ko", "ja"] {
            let tr = test_translator_for(locale);
            let by_surface = optional_target_surface(&json!({ "surface": "x" }), &tr)
                .expect_err("문자열은 surface id 가 아니다")
                .message;
            let by_surface_id = optional_target_surface(&json!({ "surface_id": "x" }), &tr)
                .expect_err("문자열은 surface id 가 아니다")
                .message;
            assert!(
                by_surface_id.contains("surface_id"),
                "{locale}: 오류에 잘못된 surface_id 필드를 포함해야 한다: {by_surface_id}"
            );
            assert!(
                !by_surface.contains("surface_id"),
                "{locale}: 오류에 surface 대신 surface_id를 표시했다: {by_surface}"
            );
            assert_ne!(
                by_surface, by_surface_id,
                "{locale}: 오류 문구로 잘못된 필드를 구분할 수 있어야 한다"
            );
        }
    }

    /// JSON null은 값이 생략된 경우로 처리한다.
    #[test]
    fn a_null_slot_reads_as_absent_not_as_a_malformed_value() {
        let tr = test_translator();
        let e = require_u32(&json!({ "surface": Value::Null }), "surface", &tr).unwrap_err();
        assert!(e.message.contains(&absent_msg(&tr, "surface")), "{e:?}");
    }

    /// 플러그인 디렉터리 환경 없이 이 크레이트의 영어 카탈로그를 읽는다.
    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    fn test_translator_for(code: &str) -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, code)
    }

    /// 지정한 메서드 호출만 실패시키는 호스트 대역.
    struct FlakyHost {
        fail: Vec<&'static str>,
        /// 같은 메서드의 이벤트별 호출을 구분하도록 파라미터도 기록한다.
        seen: RefCell<Vec<(String, Value)>>,
    }

    impl FlakyHost {
        fn failing(fail: Vec<&'static str>) -> Self {
            Self {
                fail,
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl HostCall for FlakyHost {
        fn call(
            &self,
            method: &str,
            _params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            self.seen.borrow_mut().push((method.to_string(), _params));
            if self.fail.contains(&method) {
                Err(tasty_plugin_sdk::PluginError::HostCall {
                    method: method.to_string(),
                    message: "no live surface 999".to_string(),
                    code: Some(-32602),
                })
            } else {
                Ok(json!({}))
            }
        }
    }

    #[test]
    fn session_end_closes_execution_without_synthesizing_an_idle_turn() {
        let host = FlakyHost::failing(vec![]);
        handle_hook(
            &host,
            &json!({"surface":99,"event":"session-end","session":"ending"}),
            &test_translator(),
        )
        .unwrap();
        let calls = host.seen.borrow();
        assert_eq!(calls[0].0, "surface.meta.unset");
        assert_eq!(calls[0].1["key"], "codex-session-id");
        assert!(
            !calls
                .iter()
                .any(|(method, _)| method == "terminal.set_state" || method == "surface.fire_hook")
        );
    }

    /// session-start의 메타데이터 기록 두 건이 실패하면 응답에 2를 담아야 한다.
    #[test]
    fn a_hook_response_reports_the_best_effort_calls_that_failed() {
        let host = FlakyHost::failing(vec!["surface.meta.set"]);
        let out = handle_hook(
            &host,
            &json!({ "surface_id": 999, "event": "session-start", "session": "s-1" }),
            &test_translator(),
        )
        .unwrap();

        assert_eq!(
            out["host_call_failures"],
            2,
            "시도한 호출: {:?}",
            host.seen.borrow()
        );
    }

    /// 모든 호출이 성공해도 실패 수 필드는 0으로 반환해야 한다.
    #[test]
    fn the_failure_count_is_present_even_when_nothing_failed() {
        let host = FlakyHost::failing(Vec::new());
        let out = handle_hook(
            &host,
            &json!({ "surface_id": 1, "event": "session-start", "session": "s-1" }),
            &test_translator(),
        )
        .unwrap();

        assert_eq!(out["host_call_failures"], 0);
        assert!(
            out.get("host_call_failures").is_some(),
            "실패가 0 이어도 필드는 있어야 한다"
        );
    }

    /// 상태 갱신 실패는 집계가 아니라 Err로 반환한다.
    #[test]
    fn the_propagated_call_is_an_error_not_a_counted_failure() {
        let host = FlakyHost::failing(vec!["terminal.set_state"]);
        let err = handle_hook(
            &host,
            &json!({ "surface_id": 999, "event": "stop" }),
            &test_translator(),
        );

        assert!(err.is_err(), "전파하는 호출의 실패는 Err 로 나간다");
    }

    /// 대상 형식 오류와 값 충돌을 누락 안내로 바꾸지 않아야 한다. 세 언어를 확인한다.
    #[test]
    fn a_hook_does_not_answer_malformed_or_conflicting_names_with_requires_surface() {
        for locale in ["en", "ko", "ja"] {
            let tr = test_translator_for(locale);
            let host = FlakyHost::failing(Vec::new());
            let requires = tr.t("codex.hook.requires_surface");

            let call = |params| {
                handle_hook(&host, &params, &tr)
                    .expect_err("대상 surface 를 못 정하면 Err 다")
                    .message
            };

            let by_surface = call(json!({ "surface": "x", "event": "stop" }));
            let by_surface_id = call(json!({ "surface_id": "x", "event": "stop" }));
            let conflict = call(json!({ "surface": 1, "surface_id": 2, "event": "stop" }));
            let absent = call(json!({ "event": "stop" }));

            for (label, msg) in [
                ("surface", &by_surface),
                ("surface_id", &by_surface_id),
                ("conflict", &conflict),
            ] {
                assert!(
                    !msg.contains(requires),
                    "{locale}/{label}: 잘못된 값을 누락으로 안내했다: {msg}"
                );
            }
            assert!(
                by_surface_id.contains("surface_id"),
                "{locale}: 오류에 잘못된 surface_id 필드를 포함해야 한다: {by_surface_id}"
            );
            assert!(
                !by_surface.contains("surface_id"),
                "{locale}: 오류에 surface 대신 surface_id를 표시했다: {by_surface}"
            );
            assert!(
                conflict.contains('1') && conflict.contains('2'),
                "{locale}: 충돌한 두 값을 오류에 포함해야 한다: {conflict}"
            );
            assert!(
                absent.contains(requires),
                "{locale}: 대상 누락에는 훅 전용 안내를 사용해야 한다: {absent}"
            );
        }
    }

    /// 동시 시험의 파일 이름 충돌을 줄이도록 PID와 시험 번호를 섞는다.
    fn unique_surface_id(slot: u32) -> u32 {
        std::process::id().wrapping_mul(8).wrapping_add(slot)
    }

    /// 시험 뒤 정리할 파일도 제품과 같은 경로 생성 함수를 사용한다.
    fn prompt_path(surface_id: u32) -> std::path::PathBuf {
        prompt_file::path_for(&std::env::temp_dir(), PROMPT_FILE_PREFIX, surface_id)
    }

    #[test]
    fn make_codex_command_no_prompt() {
        assert_eq!(
            make_codex_command(42, None, ""),
            "TASTY_SURFACE_ID=42 command codex --dangerously-bypass-hook-trust\r"
        );
        assert_eq!(
            make_codex_command(42, Some(""), ""),
            "TASTY_SURFACE_ID=42 command codex --dangerously-bypass-hook-trust\r"
        );
    }

    #[test]
    fn make_codex_command_with_plain_prompt_uses_tempfile_cat() {
        let surface_id = unique_surface_id(1);
        let cmd = make_codex_command(surface_id, Some("hello"), "");
        assert!(
            cmd.starts_with(&format!(
                "TASTY_SURFACE_ID={surface_id} {} --dangerously-bypass-hook-trust \"$(cat '",
                crate::POSIX_CODEX_COMMAND
            )),
            "got {cmd}"
        );
        assert!(cmd.ends_with("')\"\r"), "got {cmd}");
        // 임시 파일 삭제 실패는 시험 결과에 영향을 주지 않는다.
        let _ = std::fs::remove_file(prompt_path(surface_id));
    }

    #[test]
    fn make_codex_command_prompt_file_preserves_content_verbatim() {
        // 셸에서 특별한 의미가 있는 문자도 파일 안에서는 그대로 보존해야 한다.
        let prompt = "fix [!NOTE] \"bug\" in path\\to\\file";
        let surface_id = unique_surface_id(2);
        // 명령 문자열 대신 생성된 파일 내용을 검사한다.
        let _ = make_codex_command(surface_id, Some(prompt), "");
        let path = prompt_path(surface_id);
        let written = std::fs::read_to_string(&path).expect("prompt file should exist");
        assert_eq!(written, prompt);
        // 임시 파일 삭제 실패는 시험 결과에 영향을 주지 않는다.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn make_codex_command_with_policy_args_no_prompt() {
        assert_eq!(
            make_codex_command(42, None, "-a never -s read-only"),
            format!(
                "TASTY_SURFACE_ID=42 {} --dangerously-bypass-hook-trust -a never -s read-only\r",
                crate::POSIX_CODEX_COMMAND
            )
        );
    }

    #[test]
    fn make_codex_command_with_policy_args_and_prompt() {
        let surface_id = unique_surface_id(3);
        let cmd = make_codex_command(surface_id, Some("hello"), "-a never");
        assert!(
            cmd.starts_with(&format!(
                "TASTY_SURFACE_ID={surface_id} {} --dangerously-bypass-hook-trust -a never \"$(cat '", crate::POSIX_CODEX_COMMAND
            )),
            "got {cmd}"
        );
        // 임시 파일 삭제 실패는 시험 결과에 영향을 주지 않는다.
        let _ = std::fs::remove_file(prompt_path(surface_id));
    }

    #[test]
    fn make_codex_command_with_full_auto_bypass() {
        assert_eq!(
            make_codex_command(42, None, "--dangerously-bypass-approvals-and-sandbox"),
            format!(
                "TASTY_SURFACE_ID=42 {} --dangerously-bypass-hook-trust --dangerously-bypass-approvals-and-sandbox\r",
                crate::POSIX_CODEX_COMMAND
            )
        );
    }

    #[test]
    fn resolve_policy_args_defaults_to_never_approval_when_nothing_set() {
        // 설정이 없으면 승인은 never이고 샌드박스 플래그는 생략한다.
        let host = MockHost::new();
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never"
        );
    }

    #[test]
    fn resolve_policy_args_uses_explicit_approval_and_sandbox() {
        let host = MockHost::new();
        let params = json!({ "approval": "never", "sandbox": "read-only" });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "-a never -s read-only"
        );
    }

    #[test]
    fn resolve_policy_args_rejects_invalid_approval_value() {
        let host = MockHost::new();
        let err = resolve_policy_args(&host, &json!({ "approval": "yolo" }), &test_translator())
            .unwrap_err();
        assert!(format!("{err:?}").contains("invalid 'approval'"));
    }

    #[test]
    fn resolve_policy_args_rejects_invalid_sandbox_value() {
        let host = MockHost::new();
        let err = resolve_policy_args(&host, &json!({ "sandbox": "yolo" }), &test_translator())
            .unwrap_err();
        assert!(format!("{err:?}").contains("invalid 'sandbox'"));
    }

    #[test]
    fn resolve_policy_args_full_auto_bypasses_both() {
        let host = MockHost::new();
        let params = json!({ "full_auto": true });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "--dangerously-bypass-approvals-and-sandbox"
        );
    }

    #[test]
    fn resolve_policy_args_full_auto_rejects_combination_with_approval() {
        let host = MockHost::new();
        let params = json!({ "full_auto": true, "approval": "never" });
        let err = resolve_policy_args(&host, &params, &test_translator()).unwrap_err();
        assert!(format!("{err:?}").contains("full_auto"));
    }

    #[test]
    fn resolve_policy_args_falls_back_to_global_defaults() {
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "never");
        host.set_setting("default_sandbox_mode", "workspace-write");
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never -s workspace-write"
        );
    }

    #[test]
    fn resolve_policy_args_per_call_override_wins_over_global_default() {
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "never");
        let params = json!({ "approval": "on-request" });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "-a on-request"
        );
    }

    #[test]
    fn resolve_policy_args_global_inherit_still_falls_back_to_never() {
        // inherit도 승인 정책의 최종 기본값인 never를 사용한다.
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "inherit");
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never"
        );
    }

    /// 승인 대기는 상태 변경 외에 입력 대기 훅과 화면 알림도 보내야 한다.
    #[test]
    fn a_permission_request_raises_state_notification_and_attention_together() {
        let host = FlakyHost::failing(Vec::new());
        let out = handle_hook(
            &host,
            &json!({ "surface_id": 42, "event": "permission-request" }),
            &test_translator(),
        )
        .unwrap();
        assert_eq!(out["host_call_failures"], 0);

        let seen = host.seen.borrow();
        assert!(
            seen.contains(&(
                "terminal.set_state".to_string(),
                json!({ "surface": 42, "state": "needs_input" })
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&(
                "surface.fire_hook".to_string(),
                json!({ "surface_id": 42, "event": "needs-input" })
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&(
                "surface.completion".to_string(),
                json!({ "surface_id": 42, "kind": "needs_input" })
            )),
            "{seen:?}"
        );
    }

    /// interrupt는 idle 상태와 완료 훅을 전달해야 한다.
    #[test]
    fn an_interrupt_goes_idle_and_fires_the_completion_hook() {
        let host = FlakyHost::failing(Vec::new());
        handle_hook(
            &host,
            &json!({ "surface_id": 42, "event": "interrupt" }),
            &test_translator(),
        )
        .unwrap();

        let seen = host.seen.borrow();
        assert!(
            seen.contains(&(
                "terminal.set_state".to_string(),
                json!({ "surface": 42, "state": "idle" })
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&(
                "surface.fire_hook".to_string(),
                json!({ "surface_id": 42, "event": "codex-idle" })
            )),
            "{seen:?}"
        );
    }

    /// 도구 실행 후에는 active로만 바꾸고 알림을 추가하지 않는다.
    #[test]
    fn a_post_tool_use_only_returns_to_active() {
        let host = FlakyHost::failing(Vec::new());
        handle_hook(
            &host,
            &json!({ "surface_id": 42, "event": "post-tool-use" }),
            &test_translator(),
        )
        .unwrap();

        let seen = host.seen.borrow();
        assert_eq!(
            *seen,
            vec![(
                "terminal.set_state".to_string(),
                json!({ "surface": 42, "state": "active" })
            )],
            "{seen:?}"
        );
    }

    #[test]
    fn hook_event_to_state_maps_known_events() {
        assert_eq!(
            hook_event_to_state("stop", &test_translator()).unwrap(),
            "idle"
        );
        assert_eq!(
            hook_event_to_state("prompt-submit", &test_translator()).unwrap(),
            "active"
        );
        assert_eq!(
            hook_event_to_state("session-start", &test_translator()).unwrap(),
            "active"
        );
        assert_eq!(
            hook_event_to_state("permission-request", &test_translator()).unwrap(),
            "needs_input"
        );
        assert_eq!(
            hook_event_to_state("post-tool-use", &test_translator()).unwrap(),
            "active"
        );
        assert_eq!(
            hook_event_to_state("interrupt", &test_translator()).unwrap(),
            "idle"
        );
    }

    /// 설치한 모든 이벤트를 상태 변환에서 처리해야 한다.
    #[test]
    fn every_installed_event_has_a_state_mapping() {
        for (camel, kebab, _) in HOOK_EVENTS {
            assert!(
                hook_event_to_state(kebab, &test_translator()).is_ok(),
                "설치한 {camel} 이벤트의 {kebab} 처리가 없다"
            );
        }
    }

    #[test]
    fn hook_event_to_state_rejects_unsupported() {
        // 설치하지 않는 notification은 오류로 거부한다.
        let err = hook_event_to_state("notification", &test_translator()).unwrap_err();
        assert!(format!("{err:?}").contains("unknown hook event"));
    }

    #[test]
    fn build_spawn_warning_none_below_threshold() {
        assert_eq!(
            build_spawn_warning(&test_translator(), 3, &[], &[], 6.0),
            None
        );
    }

    #[test]
    fn build_spawn_warning_above_threshold_lists_idle_and_mentions_respawn() {
        let w = build_spawn_warning(&test_translator(), 7, &[2, 5], &[], 6.0).unwrap();
        assert!(w.contains("respawn"));
        assert!(w.contains('2') && w.contains('5'));
    }

    #[test]
    fn build_spawn_warning_above_threshold_no_idle_has_no_respawn_word() {
        let w = build_spawn_warning(&test_translator(), 7, &[], &[], 6.0).unwrap();
        assert!(!w.contains("respawn"));
    }

    #[test]
    fn build_spawn_warning_respects_custom_threshold() {
        // threshold=6 이면 안 뜨는 3개가, threshold=3 이면 뜬다(설정 override 시나리오).
        let tr = test_translator();
        assert_eq!(build_spawn_warning(&tr, 3, &[], &[], 6.0), None);
        assert!(build_spawn_warning(&tr, 4, &[], &[], 3.0).is_some());
    }

    /// stale 자식도 재시작 후보로 안내해야 한다.
    #[test]
    fn build_spawn_warning_lists_stale_children_as_respawn_candidates() {
        let w = build_spawn_warning(&test_translator(), 7, &[], &[3], 6.0).unwrap();
        assert!(w.contains("respawn"), "{w}");
        assert!(w.contains('3'), "{w}");
    }

    /// stale은 완료 보고가 없으므로 idle과 다른 문구를 사용한다.
    #[test]
    fn build_spawn_warning_separates_stale_wording_from_idle() {
        let tr = test_translator();
        let idle_only = build_spawn_warning(&tr, 7, &[2], &[], 6.0).unwrap();
        let stale_only = build_spawn_warning(&tr, 7, &[], &[3], 6.0).unwrap();
        assert!(idle_only.contains("have already finished their work"));
        assert!(
            !stale_only.contains("have already finished their work"),
            "{stale_only}"
        );
        assert!(
            stale_only.contains("never reported completion"),
            "{stale_only}"
        );

        let both = build_spawn_warning(&tr, 7, &[2], &[3], 6.0).unwrap();
        assert!(both.contains("have already finished their work"), "{both}");
        assert!(both.contains("never reported completion"), "{both}");
    }

    /// 실제 출력이 번역 카탈로그를 사용한 결과와 같아야 한다.
    #[test]
    fn build_spawn_warning_matches_lang_catalog_after_substitution() {
        let tr = test_translator();
        let expected = tr
            .t("codex.spawn_warning.total")
            .replace("{total}", "7")
            .replace("{threshold}", "6")
            + &tr
                .t("codex.spawn_warning.idle")
                .replace("{indices}", "2, 5")
            + &tr.t("codex.spawn_warning.stale").replace("{indices}", "3");
        assert_eq!(
            build_spawn_warning(&tr, 7, &[2, 5], &[3], 6.0).unwrap(),
            expected
        );
    }

    /// 언어를 바꿔도 자식 수와 인덱스는 그대로 표시해야 한다.
    #[test]
    fn build_spawn_warning_follows_active_locale() {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        let en =
            build_spawn_warning(&Translator::load(&lang_dir, "en"), 7, &[2], &[3], 6.0).unwrap();
        let ko =
            build_spawn_warning(&Translator::load(&lang_dir, "ko"), 7, &[2], &[3], 6.0).unwrap();
        let ja =
            build_spawn_warning(&Translator::load(&lang_dir, "ja"), 7, &[2], &[3], 6.0).unwrap();
        assert_ne!(en, ko);
        assert_ne!(en, ja);
        assert_ne!(ko, ja);
        // 어느 로케일이든 치환 자체는 성공해야 한다(플레이스홀더가 남지 않는다).
        for (locale, msg) in [("en", &en), ("ko", &ko), ("ja", &ja)] {
            assert!(!msg.contains("{total}"), "{locale}: {msg}");
            assert!(!msg.contains("{threshold}"), "{locale}: {msg}");
            assert!(!msg.contains("{indices}"), "{locale}: {msg}");
            assert!(
                msg.contains('7') && msg.contains('2') && msg.contains('3'),
                "{locale}: {msg}"
            );
        }
    }

    // 카탈로그 키·자리표시자·호출 정합은 tests/i18n_key_parity.rs에서 검사한다.

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

    /// 환경 값이 없으면 실행하지 않고, 명령 실패는 Codex 턴을 막지 않아야 한다.
    /// 실패 기록은 Tasty CLI가 담당하므로 안쪽 || true를 유지한다.
    #[cfg(not(windows))]
    #[test]
    fn posix_hook_command_separates_guard_from_failure_handling() {
        for (_, kebab, _) in HOOK_EVENTS {
            let cmd = hook_command(kebab);
            assert!(
                cmd.starts_with("if [ -n \"$TASTY_SURFACE_ID\" ]; then "),
                "가드가 if 블록이 아니다: {cmd}"
            );
            assert!(cmd.ends_with("; fi"), "블록이 닫히지 않았다: {cmd}");
            assert!(
                !cmd.contains("] && "),
                "옛 `A && B || true` 형태로 회귀했다: {cmd}"
            );
            // 기존 훅을 찾아 교체할 표식이 필요하다.
            assert!(cmd.contains(HOOK_MARKER), "marker 를 잃었다: {cmd}");
        }
    }

    /// 옛 명령을 다시 설치하면 항목을 늘리지 않고 새 명령으로 교체해야 한다.
    #[cfg(not(windows))]
    #[test]
    fn merge_install_replaces_legacy_command_without_growing() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "[ -n \"$TASTY_SURFACE_ID\" ] && tasty codex hook stop --surface $TASTY_SURFACE_ID || true"
"#,
        );
        let result = merge_install(initial);
        let arr = result
            .get("hooks")
            .and_then(|v| v.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(arr.len(), 1, "중복 entry 가 생기면 hook 이 두 번 발화한다");
        let cmd = arr[0]
            .get("hooks")
            .and_then(|v| v.as_array())
            .and_then(|a| a[0].get("command"))
            .and_then(|c| c.as_str())
            .unwrap();
        assert_eq!(cmd, hook_command("stop"));
    }

    /// 실제 셸에서 환경 값이 없을 때 출력 없이 성공하는지 확인한다.
    #[cfg(unix)]
    #[test]
    fn posix_guard_exits_silently_without_surface_id() {
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(hook_command("stop"))
            .env_remove("TASTY_SURFACE_ID")
            .env("PATH", "/nonexistent")
            .output()
            .expect("/bin/sh");
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(
            out.stderr.is_empty(),
            "stderr가 비어 있어야 한다: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Codex receives a no-decision response, while the CLI still receives the
    /// event, surface and stdin payload and can report diagnostics on stderr.
    #[cfg(unix)]
    #[test]
    fn posix_hook_output_is_codex_json_even_when_delivery_fails() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        for (_, event, _) in HOOK_EVENTS {
            for exit_code in [0, 1] {
                let script = format!(
                    r#"tasty() {{
    [ "$*" = "codex hook {event} --surface 42" ] || exit 91
    IFS= read -r payload
    [ "$payload" = '{{"session_id":"test-session"}}' ] || exit 92
    printf '{{"host_call_failures":2}}\n'
    printf 'delivery diagnostic\n' >&2
    return {exit_code}
}}
{}"#,
                    hook_command(event)
                );
                let mut child = Command::new("/bin/sh")
                    .args(["-c", &script])
                    .env("TASTY_SURFACE_ID", "42")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("/bin/sh");
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"{\"session_id\":\"test-session\"}\n")
                    .unwrap();
                let out = child.wait_with_output().unwrap();
                assert!(out.status.success(), "{event}, exit={exit_code}: {out:?}");
                assert_eq!(out.stdout, b"{}\n", "{event}, exit={exit_code}");
                assert_eq!(out.stderr, b"delivery diagnostic\n");
            }
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
        // Tasty 훅이 추가됐는지 확인한다.
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
    fn codex_hooks_all_trusted_in_returns_true_when_every_installed_event_present() {
        let path = "/Users/x/.codex/config.toml";
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc123"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = "sha256:def456"

[hooks.state."{path}:session_start:0:0"]
trusted_hash = "sha256:fff999"

[hooks.state."{path}:session_end:0:0"]
trusted_hash = "sha256:end777"

[hooks.state."{path}:permission_request:0:0"]
trusted_hash = "sha256:aaa111"

[hooks.state."{path}:post_tool_use:0:0"]
trusted_hash = "sha256:bbb222"

[hooks.state."{path}:interrupt:0:0"]
trusted_hash = "sha256:ccc333"
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
        // 일부 이벤트만 있고 신뢰 해시도 비어 있는 입력.
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

    use std::cell::RefCell;

    struct MockHook {
        id: u64,
        surface_id: u32,
        command: String,
        event: String,
    }

    /// 훅과 터미널 조회를 흉내 낸다. 터미널은 명시적으로 등록한 경우에만 존재한다.
    struct MockHost {
        hooks: RefCell<Vec<MockHook>>,
        next_id: RefCell<u64>,
        alive: RefCell<std::collections::HashSet<u32>>,
        settings: RefCell<std::collections::HashMap<String, String>>,
        /// None이면 화면 조회 실패, Some이면 해당 텍스트를 반환한다.
        screen_text: RefCell<Option<String>>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                hooks: RefCell::new(Vec::new()),
                next_id: RefCell::new(1),
                alive: RefCell::new(std::collections::HashSet::new()),
                settings: RefCell::new(std::collections::HashMap::new()),
                screen_text: RefCell::new(None),
            }
        }

        /// `surface.screen_text` 가 이 텍스트를 `text` 필드로 돌려주도록 세팅한다.
        fn set_screen_text(&self, text: &str) {
            *self.screen_text.borrow_mut() = Some(text.to_string());
        }

        /// `settings.get_plugin_setting` 응답을 시뮬레이션 — 전역 기본 정책
        /// (`default_approval_policy`/`default_sandbox_mode`) fallback 테스트용.
        fn set_setting(&self, storage_key: &str, value: &str) {
            self.settings
                .borrow_mut()
                .insert(storage_key.to_string(), value.to_string());
        }

        /// 발생한 이벤트와 일치하는 일회성 훅을 제거한다.
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

        /// 대상 터미널이 조회되도록 한다.
        fn mark_alive(&self, surface_id: u32) {
            self.alive.borrow_mut().insert(surface_id);
        }

        /// 대상 터미널이 조회되지 않도록 한다.
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
                "settings.get_plugin_setting" => {
                    let key = params["storage_key"].as_str().unwrap();
                    match self.settings.borrow().get(key) {
                        Some(v) => Ok(json!({ "value": v })),
                        None => Ok(json!({})),
                    }
                }
                "surface.screen_text" => match self.screen_text.borrow().as_ref() {
                    Some(text) => Ok(json!({ "text": text })),
                    None => Err(tasty_plugin_sdk::PluginError::HostCall {
                        method: method.to_string(),
                        message: "no screen_text set (mock soft-fail)".to_string(),
                        code: None,
                    }),
                },
                _ => Ok(json!({})),
            }
        }
    }

    #[test]
    fn notify_caller_message_leads_with_work_completion() {
        let msg = notify_caller_message(&test_translator_for("ko"), "spawn", 42);
        assert!(
            msg.contains("작업 완료"),
            "완료 대상이 '작업'임이 드러나야 함: {msg}"
        );
        assert!(msg.contains("42"), "target surface 번호 누락: {msg}");
        assert!(msg.contains("spawn"), "호출 방식 정보 누락: {msg}");
    }

    /// 모든 언어에서 대상 번호와 호출 방식이 치환돼야 한다.
    #[test]
    fn notify_caller_message_keeps_its_substitutions_in_every_locale() {
        for code in ["en", "ko", "ja"] {
            let msg = notify_caller_message(&test_translator_for(code), "tell", 42);
            assert!(msg.contains("42"), "[{code}] target 번호 누락: {msg}");
            assert!(msg.contains("tell"), "[{code}] 호출 방식 누락: {msg}");
            assert!(
                !msg.contains("{target}") && !msg.contains("{kind}"),
                "[{code}] 자리표시자가 치환되지 않았다: {msg}"
            );
        }
    }

    /// 언어별 카탈로그가 실제로 사용되는지 확인한다.
    #[test]
    fn notify_caller_message_changes_with_the_locale() {
        let en = notify_caller_message(&test_translator_for("en"), "spawn", 42);
        let ko = notify_caller_message(&test_translator_for("ko"), "spawn", 42);
        assert_ne!(en, ko, "언어가 달라도 같은 문구를 반환했다");
    }

    #[test]
    fn notify_caller_message_does_not_read_as_command_itself_completing() {
        // spawn/tell 접수와 자식 작업 완료를 혼동하지 않도록 한다.
        let tr = test_translator_for("ko");
        for kind in ["spawn", "tell"] {
            let msg = notify_caller_message(&tr, kind, 7);
            assert!(
                !msg.starts_with(&format!("{kind} 완료")),
                "명령 접수와 작업 완료를 구분할 수 없는 문구다: {msg}"
            );
        }
    }

    #[test]
    fn detect_sandbox_failure_hint_matches_rtm_newaddr() {
        let text = "...\nbwrap: loopback: Failed RTM_NEWADDR: Operation not permitted\n...";
        assert!(detect_sandbox_failure_hint(&test_translator(), text).is_some());
    }

    #[test]
    fn detect_sandbox_failure_hint_ignores_unrelated_text() {
        assert!(
            detect_sandbox_failure_hint(&test_translator(), "normal codex output, no errors here")
                .is_none()
        );
    }

    #[test]
    fn append_sandbox_hint_if_detected_appends_when_marker_present() {
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let text = "bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted";
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), Some(text));
        assert!(
            msg.starts_with(&base),
            "원래 완료 문구를 보존해야 한다: {msg}"
        );
        assert!(msg.contains("full-auto"), "우회법 안내 누락: {msg}");
        assert!(msg.contains("RTM_NEWADDR"), "탐지 근거 노출 누락: {msg}");
    }

    #[test]
    fn append_sandbox_hint_if_detected_unchanged_when_no_marker() {
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), Some("normal codex output"));
        assert_eq!(msg, base, "표식이 없으면 원래 완료 문구를 유지해야 한다");
    }

    #[test]
    fn append_sandbox_hint_if_detected_unchanged_when_query_failed() {
        // screen_text 조회 자체가 실패(soft-fail)한 경우 — None.
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), None);
        assert_eq!(msg, base, "조회에 실패해도 원래 완료 문구를 유지해야 한다");
    }

    #[test]
    fn notify_caller_appends_sandbox_hint_when_rtm_newaddr_detected() {
        let host = MockHost::new();
        host.set_screen_text(
            "...\nbwrap: loopback: Failed RTM_NEWADDR: Operation not permitted\n...",
        );
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        let msg = append_sandbox_hint_if_detected(&tr, base, screen_text.as_deref());
        assert!(
            msg.contains("full-auto"),
            "힌트가 실제로 덧붙어야 함: {msg}"
        );
    }

    #[test]
    fn notify_caller_message_unchanged_when_no_sandbox_failure() {
        let host = MockHost::new();
        host.set_screen_text("normal codex output, no errors here");
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), screen_text.as_deref());
        assert_eq!(
            msg, base,
            "오류 표식이 없으면 원래 완료 문구를 유지해야 한다"
        );
    }

    #[test]
    fn notify_caller_message_unchanged_when_screen_text_query_fails() {
        // 화면 조회 실패를 재현한다.
        let host = MockHost::new();
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        assert!(screen_text.is_none(), "조회 실패는 None 이어야 함");
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), screen_text.as_deref());
        assert_eq!(msg, base);
    }

    #[test]
    fn sibling_cleanup_removes_all_after_one_fires() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(
            host.commands_on(target).len(),
            3,
            "완료 훅 3개를 등록해야 한다"
        );

        // codex-idle 훅 실행 뒤 같은 그룹의 나머지 훅도 정리한다.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        let expected = notify_caller_command(caller, target, "tell");
        cleanup_sibling_hooks(&host, target, &expected);

        assert!(
            host.commands_on(target).is_empty(),
            "같은 그룹의 완료 훅이 모두 제거돼야 한다: {:?}",
            host.commands_on(target)
        );
    }

    #[test]
    fn concurrent_registrations_leave_no_zombie() {
        // 같은 자식에 spawn과 tell의 완료 훅이 겹쳐 등록된 경우.
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, caller, target, "spawn");
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(host.commands_on(target).len(), 6, "두 그룹 = 6 hook");

        // spawn 명령의 그룹만 정리한다.
        host.fire(target, "codex-idle");
        let spawn_cmd = notify_caller_command(caller, target, "spawn");
        cleanup_sibling_hooks(&host, target, &spawn_cmd);

        let remaining = host.commands_on(target);
        let tell_cmd = notify_caller_command(caller, target, "tell");
        assert!(
            remaining.iter().all(|c| c == &tell_cmd),
            "spawn 완료 훅이 남았다: {remaining:?}"
        );
        assert!(
            !remaining.iter().any(|c| c == &spawn_cmd),
            "spawn 그룹의 process-exit 훅이 남았다"
        );

        // tell 그룹도 정리한다.
        host.fire(target, "process-exit");
        cleanup_sibling_hooks(&host, target, &tell_cmd);
        assert!(
            host.commands_on(target).is_empty(),
            "최종적으로 모든 완료 훅이 제거돼야 한다: {:?}",
            host.commands_on(target)
        );
    }

    #[test]
    fn handle_notify_caller_rearms_when_target_still_alive() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, caller, target, "tell");
        assert_eq!(
            host.commands_on(target).len(),
            3,
            "처음에 완료 훅 3개를 등록해야 한다"
        );

        // 첫 알림 뒤에도 대상 터미널은 조회된다.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            &json!({ "caller": caller, "target": target, "kind": "tell" }),
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            3,
            "대상이 조회되면 완료 훅 3개를 다시 등록해야 한다"
        );

        // 다음 알림 뒤에도 다시 등록해야 한다.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            &json!({ "caller": caller, "target": target, "kind": "tell" }),
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            3,
            "두 번째 알림 뒤에도 완료 훅을 다시 등록해야 한다"
        );
    }

    #[test]
    fn handle_notify_caller_does_not_rearm_when_target_exited() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, caller, target, "spawn");

        // process-exit 알림과 함께 대상 터미널이 조회되지 않는 상황을 재현한다.
        assert_eq!(host.fire(target, "process-exit"), 1);
        host.mark_dead(target);
        handle_notify_caller(
            &host,
            &json!({ "caller": caller, "target": target, "kind": "spawn" }),
            &test_translator(),
        )
        .unwrap();

        assert!(
            host.commands_on(target).is_empty(),
            "조회되지 않는 대상의 훅을 다시 등록했다: {:?}",
            host.commands_on(target)
        );
    }

    /// children·kill에 호스트 원본 응답을 반환하는 대역.
    struct ShapeHost;

    impl HostCall for ShapeHost {
        fn call(
            &self,
            method: &str,
            _params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            match method {
                "terminal.children" => Ok(json!({ "children": [{
                    "surface_id": 42,
                    "index": 0,
                    "state": "idle",
                }]})),
                "terminal.kill" => Ok(json!({ "killed_surface_id": 42, "child_index": 0 })),
                other => panic!("unexpected host call: {other}"),
            }
        }
    }

    /// 기존 호출자 호환성을 위해 호스트의 children 객체를 그대로 반환한다.
    #[test]
    fn children_response_is_the_host_response_verbatim() {
        let out = handle_children(&ShapeHost, &json!({ "surface": 1 }), &test_translator())
            .expect("handle_children");
        assert_eq!(
            out,
            json!({ "children": [{ "surface_id": 42, "index": 0, "state": "idle" }] }),
            "호스트의 children 응답을 그대로 반환해야 한다"
        );
    }

    /// 성공 응답의 killed_surface_id·child_index를 그대로 반환해야 한다.
    #[test]
    fn kill_response_is_the_host_response_verbatim() {
        let out = handle_kill(
            &ShapeHost,
            &json!({ "surface": 1, "child": 0 }),
            &test_translator(),
        )
        .expect("handle_kill");
        assert_eq!(
            out,
            json!({ "killed_surface_id": 42, "child_index": 0 }),
            "호스트의 kill 응답을 그대로 반환해야 한다"
        );
    }
}

#[cfg(all(test, unix))]
#[path = "handlers_shell_tests.rs"]
mod shell_tests;
