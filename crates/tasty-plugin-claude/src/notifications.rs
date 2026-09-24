//! 부모 터미널에 완료·진행 정체 알림을 남긴다.
use serde_json::{Value, json};
use tasty_plugin_agent_common::host_call::{HostCall, cleanup_sibling_hooks, surface_is_alive};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

/// 완료 훅 등록과 정리에 같은 명령 문자열을 사용한다.
/// 대상 터미널과 이 문자열이 모두 같은 훅을 한 그룹으로 찾는다.
pub(crate) fn notify_done_command(
    caller_surface: u32,
    target_surface: u32,
    command_name: &str,
) -> String {
    format!(
        "tasty claude notify-done --caller-surface {caller_surface} --target-surface {target_surface} --command {command_name}"
    )
}

/// 자식의 상태 변경을 알린다. 입력 대기·종료도 포함하므로 작업 완료를 뜻하지는 않는다.
pub(crate) fn notify_done_message(
    tr: &Translator,
    command_name: &str,
    target_surface: u32,
) -> String {
    tr.t("claude.notify.done_message")
        .replacen("{}", &target_surface.to_string(), 1)
        .replacen("{}", command_name, 1)
}

/// 마지막 턴의 API 오류가 있으면 완료 문구 뒤에 덧붙인다.
pub(crate) fn with_stop_failure_hint(
    tr: &Translator,
    mut message: String,
    error: Option<&str>,
) -> String {
    if let Some(error) = error.filter(|e| !e.is_empty()) {
        message.push_str(
            &tr.t("claude.notify.stop_failure_hint")
                .replacen("{}", error, 1),
        );
    }
    message
}

/// 마지막 턴의 API 오류를 읽는다. 조회에 실패하면 힌트 없이 알린다.
fn last_stop_failure<H: HostCall>(host: &H, target_surface: u32) -> Option<String> {
    host.call(
        "surface.meta.get",
        json!({ "surface_id": target_surface, "key": crate::hook::STOP_FAILURE_META_KEY }),
    )
    .ok()
    .and_then(|r| r.get("value").and_then(|v| v.as_str()).map(String::from))
    .filter(|s| !s.is_empty())
}

/// claude-idle / needs-input / process-exit 완료 훅을 등록한다.
/// 등록 실패는 spawn/tell의 성공을 취소하지 않는다.
/// 반복해서 받는 오류 알림 훅은 완료 훅과 수명이 달라 이 그룹에 넣지 않는다.
pub(crate) fn register_notify_hooks<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
    command_name: &str,
) {
    let command = notify_done_command(caller_surface, target_surface, command_name);
    // 플러그인마다 매니페스트에 등록한 이벤트 목록을 넘긴다.
    tasty_plugin_agent_common::host_call::register_completion_hooks(
        host,
        target_surface,
        &command,
        &["claude-idle", "needs-input", "process-exit"],
        "claude",
    );
    register_error_notify_hook(host, caller_surface, target_surface);
}

/// 완료 훅과 다른 명령을 써서 완료 그룹을 정리할 때 제거되지 않게 한다.
pub(crate) fn notify_error_command(caller_surface: u32, target_surface: u32) -> String {
    format!(
        "tasty claude notify-error --caller-surface {caller_surface} --target-surface {target_surface}"
    )
}

/// 진행 정체 알림을 반복해서 받는 훅을 등록한다.
/// 발신 측 오류 감시기가 알림 빈도를 제한하므로 once를 사용하지 않는다.
/// 같은 부모·대상의 기존 훅을 정리한 뒤 새 훅 등록을 시도한다.
pub(crate) fn register_error_notify_hook<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
) {
    let command = notify_error_command(caller_surface, target_surface);
    cleanup_sibling_hooks(host, target_surface, &command);
    // 등록에 실패해도 spawn/tell은 유지하고 경고만 남긴다.
    if let Err(error) = host.call(
        "hook.set",
        json!({
            "surface_id": target_surface,
            "event": crate::error_scan::STALLED_EVENT,
            "command": command,
            "once": false,
        }),
    ) {
        tracing::warn!("completion error observation hook registration failed: {error}");
    }
}

/// 완료 로그를 남기고 같은 대상·명령의 완료 훅을 정리한다.
/// 대상이 여전히 조회되면 다음 알림을 받도록 다시 등록한다.
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

    let message = with_stop_failure_hint(
        tr,
        notify_done_message(tr, command_name, target_surface),
        last_stop_failure(host, target_surface).as_deref(),
    );
    if let Err(e) = tasty_utils::notify::append_notify_line(caller_surface, &message) {
        tracing::warn!("claude notify-done completion-log append failed: {e}");
    }

    // 다른 자식의 훅을 지우지 않도록 대상과 명령 문자열을 함께 비교한다.
    let expected_command = notify_done_command(caller_surface, target_surface, command_name);
    cleanup_sibling_hooks(host, target_surface, &expected_command);

    // 다음 상태 전환도 받을 수 있도록 대상이 조회되면 다시 등록한다.
    rearm_if_still_alive(host, caller_surface, target_surface, command_name);

    Ok(json!({}))
}

/// 진행 정체 알림을 부모 로그에 남긴다.
/// 이 알림만으로 자식 상태를 바꾸거나 완료 훅을 정리하지 않는다.
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

/// 화면의 오류 줄을 알림에 덧붙인다. 조회에 실패하면 힌트를 생략한다.
pub(crate) fn notify_error_message<H: HostCall>(
    tr: &Translator,
    host: &H,
    target_surface: u32,
) -> String {
    let screen = host
        .call(
            "surface.screen_text",
            json!({ "surface_id": target_surface }),
        )
        .ok()
        .and_then(|r| r.get("text").and_then(|t| t.as_str()).map(str::to_string));
    let error_line = screen
        .as_deref()
        .and_then(crate::error_scan::first_error_line);
    // 같은 이벤트로 들어오는 두 경우를 화면의 오류 줄 유무로 구분한다.
    let key = if error_line.is_some() {
        "claude.notify.stalled_message"
    } else {
        "claude.notify.stalled_no_error_message"
    };
    let mut message = tr.t(key).replacen("{}", &target_surface.to_string(), 1);
    if let Some(line) = error_line {
        // 알림에 넣을 오류 줄의 길이를 제한한다.
        let hint: String = line.chars().take(160).collect();
        message.push_str(&tr.t("claude.notify.stalled_hint").replacen("{}", &hint, 1));
    }
    message
}

/// 대상 터미널이 조회되면 완료 훅 3개를 다시 등록한다. 조회 실패 시 생략한다.
/// 터미널의 존재 여부만 확인하므로 자식 프로세스가 실행 중이라는 뜻은 아니다.
pub(crate) fn rearm_if_still_alive<H: HostCall>(
    host: &H,
    caller_surface: u32,
    target_surface: u32,
    command_name: &str,
) {
    if surface_is_alive(host, target_surface) {
        register_notify_hooks(host, caller_surface, target_surface, command_name);
    }
}
