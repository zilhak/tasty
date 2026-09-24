//! Claude 훅을 받아 자식 상태·세션 메타데이터·사용량 기록·화면 알림을 호스트에 요청한다.
//! 상태는 terminal.set_state로 호스트에 전달하고, 완료 알림은 별도로 등록한 훅이 처리한다.
//! 이 모듈은 부모 관계를 조회하지 않는다. 전송할 호출 목록은 apply_hook에서 만든다.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};
// 지역 HostCall enum과 이름을 구분하기 위한 호스트 호출 trait 별칭.
use tasty_plugin_agent_common::host_call::HostCall as HostCallSink;

use crate::checklist;
use crate::error_scan::ErrorScanner;
use crate::profile_attach::{self, AttachRecord};
use crate::state::ClaudeState;

/// surface meta 키 — 복원이 셸에 그대로 타이핑하는 재기동 명령
/// (`src/core/layout_persistence/restore.rs`). 값 포맷의 소유자는
/// [`crate::reboot::resume_command_line`] 다.
pub(crate) const RESTORE_COMMAND_META_KEY: &str = "restore.command";

/// surface meta 키 — 직전 턴이 API 에러로 끝났을 때 그 에러 종류(Claude Code
/// `StopFailure` payload 의 `error`: `overloaded` · `rate_limit` · `server_error` …).
/// `stop-failure` 가 쓰고 새 턴(`prompt-submit`/`session-start`/`active`)과
/// `session-end` 가 지운다. 부모 완료 알림 문구가 읽는다.
pub(crate) const STOP_FAILURE_META_KEY: &str = "claude-last-stop-failure";

/// API 에러로 턴이 끝났을 때 `claude-idle` 과 함께 쏘는 surface hook 이벤트
/// (매니페스트 `contributes.hook_events` 에 선언).
pub(crate) const STOP_FAILURE_EVENT: &str = "claude-stop-failure";

/// hook 처리 후 plugin이 호스트에 보낼 IPC 호출 1건.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    /// `terminal.set_state { surface, state }` — 호스트 registry 의 자식 상태 갱신.
    /// state ∈ {"idle", "needs_input", "active"}.
    SetState {
        surface_id: u32,
        state: &'static str,
    },
    /// `surface.fire_hook { surface_id, event }`
    FireHook {
        surface_id: u32,
        event: &'static str,
    },
    /// `surface.meta.set { surface_id, key, value }`
    MetaSet {
        surface_id: u32,
        key: &'static str,
        value: String,
    },
    /// `surface.meta.unset { surface_id, key }`
    MetaUnset { surface_id: u32, key: &'static str },
    /// `telemetry.record { metric, value, tags }`
    TelemetryRecord {
        metric: &'static str,
        value: f64,
        surface_id: u32,
    },
    /// surface.completion으로 completion 또는 needs_input 화면 알림을 요청한다.
    /// 자식 상태를 갱신하는 SetState와는 별도 호출이다.
    SurfaceCompletion { surface_id: u32, kind: &'static str },
}

pub(crate) fn handle_claude_hook(
    state: &mut ClaudeState,
    scanner: &Arc<Mutex<ErrorScanner>>,
    resume: &Mutex<crate::auto_resume::ResumeTable>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.hook.missing_event")))?;

    let surface_id = resolve_surface_id(params, tr)?;
    let session = params
        .get("session")
        .and_then(|v| v.as_str())
        .map(String::from);
    let message = params.get("message").and_then(|v| v.as_str());
    let notification_type = params.get("notification_type").and_then(|v| v.as_str());
    let error = params.get("error").and_then(|v| v.as_str());
    let agent_id = params.get("agent_id").and_then(|v| v.as_str());
    let now_ms = now_ms();

    if is_subagent_stop_failure(event, agent_id) {
        // 메인 턴은 끝나지 않았다 — 상태·알림·meta 어느 것도 건드리지 않는다.
        tracing::info!(
            "claude hook stop-failure s{surface_id}: subagent {agent_id:?} failed ({error:?}) — main turn continues, ignored"
        );
        return Ok(json!({
            "ok": true,
            "surface_id": surface_id,
            "event": event,
            "ignored": "subagent",
            "host_call_failures": 0,
        }));
    }

    let mut calls = apply_hook(
        event,
        surface_id,
        session.as_deref(),
        notification_type,
        error,
        tr,
    )?;

    // surface 메타데이터는 복원 시 유지되지 않으므로 복원 명령에도 프로필을 넣는다.
    if event == "session-start"
        && let Some(session_id) = session.as_deref()
    {
        let meta = crate::reboot::attached_profile_summary(host, surface_id);
        let plan = plan_session_start_profile(session_id, &meta, data_dir, tr);
        apply_session_start_profile(&mut calls, surface_id, session_id, &plan);
        // reboot 중 session-end가 남긴 종료 표시를 새 session-start 기록으로 갱신한다.
        if let Some(record) = &plan.restamp {
            profile_attach::store(data_dir, session_id, record);
        }
        // 종료 훅이 오지 않은 세션의 오래된 기록도 정리한다.
        profile_attach::sweep(data_dir);
    }

    calls.extend(telemetry_for_hook(
        state, event, surface_id, message, now_ms,
    ));

    // 호스트 호출의 실패 횟수를 항상 응답에 포함한다.
    let host_call_failures = deliver_all(host, &calls);
    // 상태(`idle`)를 먼저 쓴 뒤에 예약한다 — 만기 처리가 그 상태를 확인한다.
    crate::auto_resume::observe_hook(
        resume,
        host,
        event,
        surface_id,
        error,
        std::time::Instant::now(),
    );

    if is_new_turn_event(event) {
        reset_dedupe_if_enabled(scanner, surface_id);
    }

    // 게이트 판정은 별도 경로지만 세션 종료 시 모든 게이트의 회차 상태를 정리한다.
    if event == "session-end" {
        checklist::remove_state_for_session(data_dir, session.as_deref().unwrap_or(""));
        // 닫은 탭을 곧바로 복원할 수 있도록 즉시 지우지 않고 종료 표시만 남긴다.
        // reboot 뒤 session-start가 오면 새 기록으로 갱신한다.
        profile_attach::mark_ended(data_dir, session.as_deref().unwrap_or(""));
    }

    // ok는 핸들러 완료를 뜻한다. 호스트 호출의 성공 여부는 실패 횟수로 구분한다.
    Ok(json!({
        "ok": true,
        "surface_id": surface_id,
        "event": event,
        "host_call_failures": host_call_failures,
    }))
}

/// agent_id가 있는 stop-failure는 서브에이전트 오류로 처리한다.
/// 이 오류만으로 메인 세션을 idle로 바꾸거나 완료 알림을 보내지 않는다.
pub(crate) fn is_subagent_stop_failure(event: &str, agent_id: Option<&str>) -> bool {
    event == "stop-failure" && agent_id.is_some_and(|a| !a.is_empty())
}

/// 새 턴으로 처리할 이벤트. 이전 턴의 오류가 새 알림을 막지 않도록 중복 기록을 초기화한다.
fn is_new_turn_event(event: &str) -> bool {
    matches!(event, "prompt-submit" | "session-start" | "active")
}

/// 오류 감시 중인 surface에만 중복 기록 초기화를 적용한다.
fn reset_dedupe_if_enabled(scanner: &Arc<Mutex<ErrorScanner>>, surface_id: u32) {
    let mut s = crate::error_scan::lock_scanner(scanner);
    if s.is_enabled(surface_id) {
        s.reset_dedupe(surface_id);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// session-start에서 시각을 기록하고 종료 이벤트에서 경과 시간을 계산한다.
/// notification에 토큰 사용량이 있으면 input_tokens 기록도 만든다.
pub(crate) fn telemetry_for_hook(
    state: &mut ClaudeState,
    event: &str,
    surface_id: u32,
    message: Option<&str>,
    now_ms: u64,
) -> Vec<HostCall> {
    let mut out = Vec::new();
    match event {
        "session-start" => {
            state.mark_session_start(surface_id, now_ms);
        }
        "stop" | "stop-failure" | "subagent-stop" | "session-end" => {
            if let Some(elapsed) = state.take_wall_time(surface_id, now_ms) {
                out.push(HostCall::TelemetryRecord {
                    metric: "wall_time_ms",
                    value: elapsed as f64,
                    surface_id,
                });
            }
        }
        "notification" => {
            if let Some(text) = message
                && let Some(n) = extract_tokens(text)
            {
                out.push(HostCall::TelemetryRecord {
                    metric: "input_tokens",
                    value: n as f64,
                    surface_id,
                });
            }
        }
        _ => {}
    }
    out
}

/// token/Token에 선택적인 s/S, 콜론, 공백·탭, 숫자가 이어지는 표현을 찾는다.
/// 앞뒤는 ASCII 영숫자가 아니어야 하며 정규식의 Unicode 단어 경계와는 다르다.
fn extract_tokens(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 시작 위치이거나 앞 바이트가 ASCII 영숫자가 아니어야 한다.
        let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if prev_ok && (bytes[i..].starts_with(b"token") || bytes[i..].starts_with(b"Token")) {
            let mut j = i + 5;
            if j < bytes.len() && (bytes[j] == b's' || bytes[j] == b'S') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start {
                    let after_ok = j == bytes.len() || !bytes[j].is_ascii_alphanumeric();
                    if after_ok
                        && let Ok(n) = std::str::from_utf8(&bytes[start..j])
                            .unwrap()
                            .parse::<u64>()
                    {
                        return Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// 훅 이벤트를 호스트 호출 목록으로 바꾼다.
/// notification은 idle_prompt만 제외하고 나머지 값·누락은 needs_input으로 처리한다.
pub(crate) fn apply_hook(
    event: &str,
    surface_id: u32,
    session: Option<&str>,
    notification_type: Option<&str>,
    error: Option<&str>,
    tr: &Translator,
) -> Result<Vec<HostCall>, IpcMethodError> {
    let mut calls = Vec::new();
    // 새 턴·세션 종료에서는 이전 API 오류 기록을 지워 다음 완료 알림에 남지 않게 한다.
    if is_new_turn_event(event) || event == "session-end" {
        calls.push(HostCall::MetaUnset {
            surface_id,
            key: STOP_FAILURE_META_KEY,
        });
    }
    match event {
        "stop-failure" => {
            // 오류로 끝난 턴도 idle과 완료 이벤트로 알린다.
            // 알림 명령이 오류 종류를 읽을 수 있도록 메타데이터를 먼저 기록한다.
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::MetaSet {
                surface_id,
                key: STOP_FAILURE_META_KEY,
                // 오류 종류가 없으면 unknown으로 기록한다.
                value: error
                    .filter(|e| !e.is_empty())
                    .unwrap_or("unknown")
                    .to_string(),
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: STOP_FAILURE_EVENT,
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "completion",
            });
        }
        "stop" | "subagent-stop" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "completion",
            });
        }
        "session-end" => {
            // session-start와 순서를 대조할 수 있도록 메타데이터 정리를 기록한다.
            tracing::info!(
                "claude hook session-end s{surface_id}: clearing claude-session-id/restore.command meta"
            );
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: "claude-session-id",
            });
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: RESTORE_COMMAND_META_KEY,
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "completion",
            });
        }
        "notification" => {
            // idle_prompt만 제외하고 다른 알림은 입력 대기로 처리한다.
            if notification_type != Some("idle_prompt") {
                calls.push(HostCall::SetState {
                    surface_id,
                    state: "needs_input",
                });
                calls.push(HostCall::FireHook {
                    surface_id,
                    event: "needs-input",
                });
                calls.push(HostCall::SurfaceCompletion {
                    surface_id,
                    kind: "needs_input",
                });
            }
        }
        "pre-tool-use" => {
            // 설치된 훅의 matcher는 AskUserQuestion이다. 질문 전 입력 대기로 표시한다.
            calls.push(HostCall::SetState {
                surface_id,
                state: "needs_input",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "needs-input",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "needs_input",
            });
        }
        "post-tool-use" => {
            // AskUserQuestion 응답 뒤 자식 상태를 active로 바꾼다.
            // 화면 알림을 지우는 호출은 여기서 하지 않는다.
            calls.push(HostCall::SetState {
                surface_id,
                state: "active",
            });
        }
        "prompt-submit" | "session-start" | "active" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "active",
            });
            if event == "session-start" {
                match session {
                    Some(session_id) => {
                        calls.push(HostCall::MetaSet {
                            surface_id,
                            key: "claude-session-id",
                            value: session_id.to_string(),
                        });
                        calls.push(HostCall::MetaSet {
                            surface_id,
                            key: RESTORE_COMMAND_META_KEY,
                            // 호출자가 프로필을 확인하면 settings를 포함한 명령으로 바꾼다.
                            value: crate::reboot::resume_command_line(session_id, None),
                        });
                    }
                    None => {
                        // 세션 id가 없으면 이후 reboot에 필요한 메타데이터를 기록할 수 없다.
                        tracing::warn!(
                            "claude hook session-start s{surface_id}: no session_id in payload — claude-session-id meta NOT set (tasty claude reboot will fail until the next session-start carries a session_id)"
                        );
                    }
                }
            }
        }
        other => {
            return Err(IpcMethodError::invalid_params(
                &tr.t_fmt("claude.hook.unknown_event", other),
            ));
        }
    }
    Ok(calls)
}

/// session-start 가 복원할 프로필 계획.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SessionStartProfile {
    /// 저장 기록에서 복구했을 때 메타데이터를 되돌릴 호출 목록.
    pub meta_calls: Vec<HostCall>,
    /// 이번 세션에 실제로 실을 프로필 파일 경로. `None` 이면 프로필 없이 진행한다.
    pub profile_file: Option<String>,
    /// 프로필을 확인한 뒤 다시 저장할 기록.
    pub restamp: Option<AttachRecord>,
}

/// surface 메타데이터를 먼저 보고 없으면 저장된 프로필 기록을 읽는다.
/// 등록 이름은 매번 다시 해석해 변경된 내용을 반영한다.
/// 실패하면 경고 후 프로필 없이 복원 명령을 남긴다. 프로필 문제로 세션 복원까지 막지 않기 위해서다.
pub(crate) fn plan_session_start_profile(
    session_id: &str,
    meta: &crate::reboot::AttachedProfile,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> SessionStartProfile {
    let (attached, from_record) = match (&meta.names, &meta.path) {
        (Some(names), _) => (Some(AttachRecord::Names(names.clone())), false),
        (None, Some(path)) => (Some(AttachRecord::Path(path.clone())), false),
        (None, None) => (profile_attach::load(data_dir, session_id), true),
    };
    let Some(record) = attached else {
        return SessionStartProfile::default();
    };

    let resolved = match &record {
        AttachRecord::Names(names) => {
            match crate::profile::resolve_names(data_dir, names, tr) {
                Ok(path) => Some(path.to_string_lossy().into_owned()),
                Err(e) => {
                    // 부착 후 unregister 됐거나 파일이 깨진 경우.
                    tracing::warn!(
                        "claude hook session-start {session_id}: profile names {names:?} no longer resolve ({e:?}) — restoring without a profile"
                    );
                    None
                }
            }
        }
        AttachRecord::Path(path) => match crate::reboot::validate_profile_file(path, tr) {
            Ok(()) => Some(path.clone()),
            Err(e) => {
                tracing::warn!(
                    "claude hook session-start {session_id}: attached profile file {path:?} is unusable ({}) — restoring without a profile",
                    e.message
                );
                None
            }
        },
    };

    let Some(profile_file) = resolved else {
        // 실패 시 기존 메타데이터와 기록은 유지해 다음 session-start에서 다시 시도할 수 있게 한다.
        return SessionStartProfile::default();
    };

    let meta_calls = if from_record {
        vec![match &record {
            AttachRecord::Names(names) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_NAMES_META_KEY,
                value: names.clone(),
            },
            AttachRecord::Path(path) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_META_KEY,
                value: path.clone(),
            },
        }]
    } else {
        Vec::new()
    };

    SessionStartProfile {
        meta_calls,
        profile_file: Some(profile_file),
        restamp: Some(record),
    }
}

/// 복원 명령에 settings를 반영하고 기록에서 복구할 메타데이터 호출을 덧붙인다.
pub(crate) fn apply_session_start_profile(
    calls: &mut Vec<HostCall>,
    surface_id: u32,
    session_id: &str,
    plan: &SessionStartProfile,
) {
    if let Some(path) = &plan.profile_file {
        for call in calls.iter_mut() {
            if let HostCall::MetaSet { key, value, .. } = call
                && *key == RESTORE_COMMAND_META_KEY
            {
                *value = crate::reboot::resume_command_line(session_id, Some(path));
            }
        }
    }
    calls.extend(plan.meta_calls.iter().map(|call| match call {
        // 프로필 선택 단계에서 알 수 없던 대상 surface를 채운다.
        HostCall::MetaSet { key, value, .. } => HostCall::MetaSet {
            surface_id,
            key,
            value: value.clone(),
        },
        other => other.clone(),
    }));
}

/// 계획한 호스트 호출을 모두 시도하고 실패한 수를 반환한다.
/// 실패를 전파해 뒤의 로컬 파일 정리를 건너뛰지 않도록 한다.
/// 종료 훅은 surface가 이미 사라진 뒤에도 도착할 수 있지만 세션 파일 정리는 여전히 필요하다.
/// 정책은 docs/dev-guide/plugin-development.md#실패-후-로컬-정리 참고.
fn deliver_all<H: HostCallSink>(host: &H, calls: &[HostCall]) -> usize {
    calls.iter().filter(|call| !deliver(host, call)).count()
}

fn deliver<H: HostCallSink>(host: &H, call: &HostCall) -> bool {
    let (method, params) = match call {
        HostCall::SetState { surface_id, state } => (
            "terminal.set_state",
            json!({ "surface": surface_id, "state": state }),
        ),
        HostCall::FireHook { surface_id, event } => (
            "surface.fire_hook",
            json!({ "surface_id": surface_id, "event": event }),
        ),
        HostCall::MetaSet {
            surface_id,
            key,
            value,
        } => (
            "surface.meta.set",
            json!({ "surface_id": surface_id, "key": key, "value": value }),
        ),
        HostCall::MetaUnset { surface_id, key } => (
            "surface.meta.unset",
            json!({ "surface_id": surface_id, "key": key }),
        ),
        HostCall::TelemetryRecord {
            metric,
            value,
            surface_id,
        } => (
            "telemetry.record",
            json!({
                "metric": metric,
                "value": value,
                "tags": { "surface_id": surface_id.to_string() },
            }),
        ),
        HostCall::SurfaceCompletion { surface_id, kind } => (
            "surface.completion",
            json!({ "surface_id": surface_id, "kind": kind }),
        ),
    };
    if let Err(e) = host.call(method, params) {
        tracing::warn!("claude hook host call '{method}' failed: {e}");
        return false;
    }
    true
}

/// surface·surface_id를 공용 판정으로 읽고 생략한 경우에만 전달받은 환경 값으로 대신한다.
/// 환경을 직접 읽지 않아 시험에서 값을 제어할 수 있다.
fn resolve_surface_id_from(
    params: &Value,
    env_surface: Option<&str>,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    // 잘못 지정한 값은 오류로 반환하고, 값이 없을 때만 환경 값으로 대신한다.
    if let Some(sid) = crate::handlers::optional_target_surface(params, tr)? {
        return Ok(sid);
    }
    if let Some(sid) = env_surface.and_then(|e| e.parse::<u32>().ok()) {
        return Ok(sid);
    }
    Err(IpcMethodError::invalid_params(
        tr.t("claude.params.missing_surface_no_env"),
    ))
}

/// 실제 TASTY_SURFACE_ID를 읽어 대상 판정에 전달한다.
fn resolve_surface_id(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    let env = std::env::var("TASTY_SURFACE_ID").ok();
    resolve_surface_id_from(params, env.as_deref(), tr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 카탈로그로 오류 안내를 확인한다.
    fn test_translator() -> Translator {
        test_translator_for("en")
    }

    fn test_translator_for(code: &str) -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, code)
    }

    /// 지정한 메서드만 실패시키는 mock 호스트.
    struct FlakyHost {
        /// 실패시킬 method 이름. 비면 전부 성공.
        fail: Vec<&'static str>,
        seen: std::cell::RefCell<Vec<String>>,
    }

    impl FlakyHost {
        fn failing_everything() -> Self {
            Self {
                fail: vec![
                    "terminal.set_state",
                    "surface.fire_hook",
                    "surface.meta.set",
                    "surface.meta.unset",
                    "telemetry.record",
                    "surface.completion",
                ],
                seen: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn healthy() -> Self {
            Self {
                fail: Vec::new(),
                seen: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl HostCallSink for FlakyHost {
        fn call(
            &self,
            method: &str,
            _params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            self.seen.borrow_mut().push(method.to_string());
            if self.fail.contains(&method) {
                // 호스트의 대상 부재 오류를 모의한다.
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

    /// 시작·종료의 호스트 호출이 모두 실패해도 열 개를 모두 시도하고 실패 횟수를 반환해야 한다.
    #[test]
    fn a_dead_surface_makes_every_host_call_fail_and_the_count_is_ten() {
        let tr = test_translator();
        let host = FlakyHost::failing_everything();

        let start = apply_hook("session-start", 100, Some("sess-1"), None, None, &tr).unwrap();
        let end = apply_hook("session-end", 100, None, None, None, &tr).unwrap();
        let failures = deliver_all(&host, &start) + deliver_all(&host, &end);

        assert_eq!(failures, 10, "시도한 호출: {:?}", host.seen.borrow());
        assert_eq!(
            host.seen.borrow().len(),
            10,
            "실패 횟수와 시도한 호출 수가 같아야 한다"
        );
    }

    /// 같은 호출이 모두 성공하면 실패 횟수는 0이어야 한다.
    #[test]
    fn a_live_host_makes_the_same_two_events_report_zero_failures() {
        let tr = test_translator();
        let host = FlakyHost::healthy();

        let start = apply_hook("session-start", 100, Some("sess-1"), None, None, &tr).unwrap();
        let end = apply_hook("session-end", 100, None, None, None, &tr).unwrap();
        let failures = deliver_all(&host, &start) + deliver_all(&host, &end);

        assert_eq!(failures, 0);
        assert_eq!(
            host.seen.borrow().len(),
            10,
            "실패가 없어도 같은 호출을 모두 시도해야 한다"
        );
    }

    /// 일부 호출이 실패해도 나머지를 계속 시도하고 실패한 수만 센다.
    #[test]
    fn only_the_failing_calls_are_counted() {
        let tr = test_translator();
        let host = FlakyHost {
            fail: vec!["surface.meta.unset"],
            seen: std::cell::RefCell::new(Vec::new()),
        };

        let end = apply_hook("session-end", 100, None, None, None, &tr).unwrap();
        assert_eq!(
            deliver_all(&host, &end),
            3,
            "meta.unset 은 이 계획에 셋이다"
        );
        assert_eq!(
            host.seen.borrow().len(),
            6,
            "실패 뒤에도 나머지 호출을 시도해야 한다"
        );
    }

    #[test]
    fn stop_sets_idle_and_emits_fire_hook() {
        let calls = apply_hook("stop", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "idle",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "completion",
                },
            ]
        );
    }

    #[test]
    fn subagent_stop_treated_like_stop() {
        let calls = apply_hook("subagent-stop", 7, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 7,
                    state: "idle",
                },
                HostCall::FireHook {
                    surface_id: 7,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 7,
                    kind: "completion",
                },
            ]
        );
    }

    #[test]
    fn notification_sets_needs_input_and_fires_needs_input() {
        let calls = apply_hook("notification", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn notification_permission_prompt_sets_needs_input() {
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("permission_prompt"),
            None,
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn notification_idle_prompt_does_not_set_needs_input() {
        // idle_prompt는 입력 대기 상태를 만들지 않는다.
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("idle_prompt"),
            None,
            &test_translator(),
        )
        .unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn notification_unknown_type_still_sets_needs_input() {
        // idle_prompt 이외의 값은 입력 대기로 처리한다.
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("auth_success"),
            None,
            &test_translator(),
        )
        .unwrap();
        assert!(!calls.is_empty());
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SetState {
                state: "needs_input",
                ..
            }
        )));
    }

    #[test]
    fn notification_missing_type_still_sets_needs_input() {
        // notification_type이 없어도 입력 대기로 처리한다.
        let calls = apply_hook("notification", 100, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SetState {
                state: "needs_input",
                ..
            }
        )));
    }

    #[test]
    fn pre_tool_use_sets_needs_input_and_fires_needs_input() {
        // AskUserQuestion 전에 상태·훅·화면 알림을 요청한다.
        let calls = apply_hook("pre-tool-use", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn post_tool_use_sets_active() {
        // 질문 응답 뒤 상태만 active로 바꾸고 새 화면 알림은 만들지 않는다.
        let calls = apply_hook("post-tool-use", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_end_clears_session_meta_and_fires_idle() {
        let calls = apply_hook("session-end", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: STOP_FAILURE_META_KEY,
                },
                HostCall::SetState {
                    surface_id: 100,
                    state: "idle",
                },
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: "claude-session-id",
                },
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: "restore.command",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "completion",
                },
            ]
        );
    }

    // 완료·입력 대기 이벤트의 화면 알림 호출을 각각 확인한다.

    #[test]
    fn stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("stop", 100, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn subagent_stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("subagent-stop", 7, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 7,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn session_end_also_raises_surface_completion_highlight() {
        let calls = apply_hook("session-end", 100, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn notification_also_raises_surface_completion_highlight() {
        let calls = apply_hook("notification", 100, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "needs_input",
            }
        )));
    }

    /// 질문 전에는 needs_input 화면 알림을 요청한다.
    #[test]
    fn pre_tool_use_also_raises_surface_completion_with_needs_input_kind() {
        let calls = apply_hook("pre-tool-use", 100, None, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "needs_input",
            }
        )));
    }

    #[test]
    fn prompt_submit_does_not_raise_surface_completion_highlight() {
        // 새 턴 시작 자체는 완료·입력 대기 화면 알림 대상이 아니다.
        let calls = apply_hook("prompt-submit", 100, None, None, None, &test_translator()).unwrap();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { .. }))
        );
    }

    #[test]
    fn prompt_submit_sets_active_and_only_clears_the_stop_failure_record() {
        let calls = apply_hook("prompt-submit", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: STOP_FAILURE_META_KEY,
                },
                HostCall::SetState {
                    surface_id: 100,
                    state: "active",
                },
            ]
        );
    }

    #[test]
    fn session_start_without_session_id_just_sets_active() {
        let calls = apply_hook("session-start", 100, None, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: STOP_FAILURE_META_KEY,
                },
                HostCall::SetState {
                    surface_id: 100,
                    state: "active",
                },
            ]
        );
    }

    #[test]
    fn only_a_stop_failure_carrying_an_agent_id_is_a_subagent_failure() {
        assert!(is_subagent_stop_failure("stop-failure", Some("a1b2")));
        assert!(!is_subagent_stop_failure("stop-failure", None));
        assert!(!is_subagent_stop_failure("stop-failure", Some("")));
        // 다른 이벤트는 agent_id 가 있어도 이 판정과 무관하다.
        assert!(!is_subagent_stop_failure("stop", Some("a1b2")));
    }

    #[test]
    fn stop_failure_sets_idle_and_fires_idle_and_stop_failure() {
        let calls = apply_hook(
            "stop-failure",
            100,
            None,
            None,
            Some("overloaded"),
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "idle",
                },
                // meta 가 fire 보다 먼저다 — 완료 알림 command 가 fire 뒤에 이것을 읽는다.
                HostCall::MetaSet {
                    surface_id: 100,
                    key: STOP_FAILURE_META_KEY,
                    value: "overloaded".into(),
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "claude-idle",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: STOP_FAILURE_EVENT,
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "completion",
                },
            ]
        );
    }

    #[test]
    fn stop_failure_without_error_field_records_unknown() {
        for error in [None, Some("")] {
            let calls =
                apply_hook("stop-failure", 5, None, None, error, &test_translator()).unwrap();
            assert!(calls.contains(&HostCall::MetaSet {
                surface_id: 5,
                key: STOP_FAILURE_META_KEY,
                value: "unknown".into(),
            }));
        }
    }

    #[test]
    fn stop_failure_record_is_cleared_by_new_turn_and_session_end_only() {
        let clears = |event: &str| {
            apply_hook(event, 9, None, None, None, &test_translator())
                .unwrap()
                .contains(&HostCall::MetaUnset {
                    surface_id: 9,
                    key: STOP_FAILURE_META_KEY,
                })
        };
        for event in ["prompt-submit", "session-start", "active", "session-end"] {
            assert!(clears(event), "{event} 는 기록을 지워야 한다");
        }
        // 에러로 끝난 턴 뒤의 알림·질문이 기록을 지우면 부모 알림이 사유를 잃는다.
        for event in ["stop", "subagent-stop", "notification", "pre-tool-use"] {
            assert!(!clears(event), "{event} 는 기록을 건드리지 않는다");
        }
    }

    #[test]
    fn stop_failure_closes_wall_time_like_stop() {
        let mut state = ClaudeState::default();
        telemetry_for_hook(&mut state, "session-start", 3, None, 1_000);
        let calls = telemetry_for_hook(&mut state, "stop-failure", 3, None, 4_000);
        assert_eq!(
            calls,
            vec![HostCall::TelemetryRecord {
                metric: "wall_time_ms",
                value: 3_000.0,
                surface_id: 3,
            }]
        );
    }

    #[test]
    fn session_start_with_session_id_emits_meta_set() {
        let calls = apply_hook(
            "session-start",
            100,
            Some("sess-abc"),
            None,
            None,
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: STOP_FAILURE_META_KEY,
                },
                HostCall::SetState {
                    surface_id: 100,
                    state: "active",
                },
                HostCall::MetaSet {
                    surface_id: 100,
                    key: "claude-session-id",
                    value: "sess-abc".into(),
                },
                HostCall::MetaSet {
                    surface_id: 100,
                    key: "restore.command",
                    value: "claude -r sess-abc".into(),
                },
            ]
        );
    }

    fn register_profile(dir: &std::path::Path, name: &str, body: &str) {
        let src = dir.join(format!("{name}-src.json"));
        std::fs::write(&src, body).unwrap();
        crate::profile::register(Some(dir), name, &src).unwrap();
    }

    fn register_gate(dir: &std::path::Path, name: &str) {
        let body = dir.join(format!("{name}-body.md"));
        std::fs::write(&body, format!("{name} 본문\n[[{name}-DONE]]\n")).unwrap();
        crate::gate::register(
            Some(dir),
            name,
            &body,
            Some(&format!("[[{name}-DONE]]")),
            Some(2),
        )
        .unwrap();
    }

    fn no_meta() -> crate::reboot::AttachedProfile {
        crate::reboot::AttachedProfile {
            names: None,
            path: None,
        }
    }

    fn names_meta(names: &str) -> crate::reboot::AttachedProfile {
        crate::reboot::AttachedProfile {
            names: Some(names.to_string()),
            path: None,
        }
    }

    /// session-start 호출 목록에서 `restore.command` 값을 뽑는다.
    fn restore_command(calls: &[HostCall]) -> Option<String> {
        calls.iter().find_map(|c| match c {
            HostCall::MetaSet { key, value, .. } if *key == RESTORE_COMMAND_META_KEY => {
                Some(value.clone())
            }
            _ => None,
        })
    }

    /// apply_hook의 호출 목록에 프로필 복원 계획을 반영한다.
    fn session_start_calls(
        surface_id: u32,
        session_id: &str,
        meta: &crate::reboot::AttachedProfile,
        data_dir: &std::path::Path,
    ) -> (Vec<HostCall>, SessionStartProfile) {
        let tr = test_translator();
        let mut calls = apply_hook(
            "session-start",
            surface_id,
            Some(session_id),
            None,
            None,
            &tr,
        )
        .unwrap();
        let plan = plan_session_start_profile(session_id, meta, Some(data_dir), &tr);
        apply_session_start_profile(&mut calls, surface_id, session_id, &plan);
        (calls, plan)
    }

    /// 메타데이터가 없으면 저장 기록으로 프로필을 복원하고 명령에 settings를 넣는다.
    #[test]
    fn session_start_restores_the_profile_from_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        let generated = plan.profile_file.expect("프로필이 해석돼야 한다");
        // OS별 경로 구분자 차이는 Path로 비교한다.
        let expected = "profiles/generated/reviewer.json";
        assert!(
            std::path::Path::new(&generated).ends_with(expected),
            "생성 프로필 경로가 `{expected}` 로 끝나야 하는데 실제로는 `{generated}` 다"
        );
        assert_eq!(
            restore_command(&calls).unwrap(),
            format!("claude -r sess-1 --settings \"{generated}\"")
        );
        // meta 도 함께 되살아난다 — 이후 `profile-current` / 무인자 reboot 가 승계한다.
        assert!(calls.contains(&HostCall::MetaSet {
            surface_id: 7,
            key: crate::reboot::PROFILE_NAMES_META_KEY,
            value: "reviewer".into(),
        }));
    }

    /// 저장 기록과 달라도 현재 메타데이터를 우선한다.
    #[test]
    fn session_start_prefers_meta_over_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("ghost".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &names_meta("reviewer"), tmp.path());

        assert!(plan.profile_file.unwrap().ends_with("reviewer.json"));
        assert!(restore_command(&calls).unwrap().contains("--settings"));
        // meta 가 이미 있으므로 meta 복구 호출은 없다.
        assert!(plan.meta_calls.is_empty());
        // 현재 메타데이터로 저장 기록도 갱신한다.
        assert_eq!(plan.restamp, Some(AttachRecord::Names("reviewer".into())));
    }

    /// 등록이 제거된 프로필은 복원에서 생략하고 세션 명령은 유지한다.
    #[test]
    fn session_start_degrades_when_the_recorded_name_no_longer_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("ghost".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        assert_eq!(plan, SessionStartProfile::default());
        assert_eq!(restore_command(&calls).unwrap(), "claude -r sess-1");
    }

    /// 경로로 지정한 파일이 사라져도 프로필 없이 복원한다.
    #[test]
    fn session_start_degrades_when_the_recorded_path_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Path(tmp.path().join("gone.json").display().to_string()),
        );

        let (calls, _) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        assert_eq!(restore_command(&calls).unwrap(), "claude -r sess-1");
    }

    /// 종료 표시 직후에는 프로필 기록을 남겨 닫은 탭 복원에 사용할 수 있어야 한다.
    #[test]
    fn session_end_marks_the_record_ended_but_keeps_it_for_a_closed_tab_restore() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "probe", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("probe".into()),
        );
        profile_attach::mark_ended(Some(tmp.path()), "sess-1");

        // 되살아난 세션의 session-start 가 기록으로 프로필을 복구할 수 있어야 한다.
        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        assert!(plan.profile_file.is_some());
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    /// 재등록한 프로필은 다음 session-start의 생성 파일에 반영돼야 한다.
    #[test]
    fn session_start_reresolves_names_against_the_current_registry() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"VERSION":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let generated = std::path::PathBuf::from(plan.profile_file.unwrap());
        let v1: Value =
            serde_json::from_str(&std::fs::read_to_string(&generated).unwrap()).unwrap();
        assert_eq!(v1["env"]["VERSION"], "1");

        register_profile(tmp.path(), "reviewer", r#"{"env":{"VERSION":"2"}}"#);
        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let generated = std::path::PathBuf::from(plan.profile_file.unwrap());
        let v2: Value =
            serde_json::from_str(&std::fs::read_to_string(&generated).unwrap()).unwrap();
        assert_eq!(v2["env"]["VERSION"], "2");
    }

    /// reboot 중 종료 표시가 생겨도 새 session-start에서 프로필 기록을 갱신해야 한다.
    #[test]
    fn reboot_then_session_end_then_session_start_keeps_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);

        // reboot가 프로필 메타데이터와 기록을 남긴다.
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        // session-end는 종료 표시를 남기고 프로필 메타데이터는 유지한다.
        let end_calls = apply_hook(
            "session-end",
            7,
            Some("sess-1"),
            None,
            None,
            &test_translator(),
        )
        .unwrap();
        assert!(!end_calls.iter().any(|c| matches!(
            c,
            HostCall::MetaUnset { key, .. }
                if *key == crate::reboot::PROFILE_NAMES_META_KEY
                    || *key == crate::reboot::PROFILE_META_KEY
        )));
        profile_attach::mark_ended(Some(tmp.path()), "sess-1");

        // 다음 session-start는 남은 메타데이터로 프로필을 다시 저장한다.
        let (calls, plan) = session_start_calls(7, "sess-1", &names_meta("reviewer"), tmp.path());
        if let Some(record) = &plan.restamp {
            profile_attach::store(Some(tmp.path()), "sess-1", record);
        }

        assert_eq!(
            profile_attach::load(Some(tmp.path()), "sess-1"),
            Some(AttachRecord::Names("reviewer".into())),
            "session-start 뒤 프로필 기록이 다시 저장돼야 한다"
        );
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    /// 같은 이름을 프로필에서 게이트로 다시 등록해도 최신 정의로 복원해야 한다.
    #[test]
    fn session_start_reinterprets_a_name_that_changed_from_profile_to_gate() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "shared", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("shared".into()),
        );

        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let as_profile: Value =
            serde_json::from_str(&std::fs::read_to_string(plan.profile_file.unwrap()).unwrap())
                .unwrap();
        assert_eq!(as_profile["env"]["A"], "1");
        assert!(as_profile.get("hooks").is_none());

        crate::profile::unregister(Some(tmp.path()), "shared").unwrap();
        register_gate(tmp.path(), "shared");

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let as_gate: Value =
            serde_json::from_str(&std::fs::read_to_string(plan.profile_file.unwrap()).unwrap())
                .unwrap();
        assert!(
            as_gate["hooks"]["Stop"].is_array(),
            "게이트로 재등록됐으면 Stop 훅 조각으로 해석돼야 한다: {as_gate}"
        );
        // 게이트 이름으로 부착한 것도 같은 경로로 복원된다.
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    #[test]
    fn unknown_event_returns_invalid_params() {
        let err = apply_hook("bogus", 100, None, None, None, &test_translator()).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("bogus"));
    }

    #[test]
    fn resolve_surface_id_prefers_explicit_param() {
        let tr = test_translator();
        assert_eq!(
            resolve_surface_id(&json!({ "surface": 42 }), &tr).unwrap(),
            42
        );
        assert_eq!(
            resolve_surface_id(&json!({ "surface_id": 7 }), &tr).unwrap(),
            7
        );
    }

    #[test]
    fn telemetry_session_start_marks_then_stop_emits_wall_time() {
        let mut state = ClaudeState::default();
        let started = telemetry_for_hook(&mut state, "session-start", 7, None, 1_000);
        assert!(started.is_empty(), "session-start emits no host calls");
        let stopped = telemetry_for_hook(&mut state, "stop", 7, None, 5_000);
        assert_eq!(
            stopped,
            vec![HostCall::TelemetryRecord {
                metric: "wall_time_ms",
                value: 4_000.0,
                surface_id: 7,
            }]
        );
        // 두번째 stop 은 start 가 없으므로 발행 안 함.
        let again = telemetry_for_hook(&mut state, "stop", 7, None, 9_000);
        assert!(again.is_empty());
    }

    #[test]
    fn telemetry_session_end_also_emits_wall_time() {
        let mut state = ClaudeState::default();
        telemetry_for_hook(&mut state, "session-start", 1, None, 100);
        let calls = telemetry_for_hook(&mut state, "session-end", 1, None, 250);
        assert_eq!(
            calls,
            vec![HostCall::TelemetryRecord {
                metric: "wall_time_ms",
                value: 150.0,
                surface_id: 1,
            }]
        );
    }

    #[test]
    fn telemetry_notification_extracts_tokens() {
        let mut state = ClaudeState::default();
        let calls = telemetry_for_hook(
            &mut state,
            "notification",
            42,
            Some("Claude used tokens: 12345 in this turn"),
            0,
        );
        assert_eq!(
            calls,
            vec![HostCall::TelemetryRecord {
                metric: "input_tokens",
                value: 12345.0,
                surface_id: 42,
            }]
        );
    }

    #[test]
    fn telemetry_notification_no_match_no_record() {
        let mut state = ClaudeState::default();
        let calls = telemetry_for_hook(&mut state, "notification", 1, Some("approval needed"), 0);
        assert!(calls.is_empty());
    }

    #[test]
    fn extract_tokens_variants() {
        assert_eq!(extract_tokens("tokens: 99"), Some(99));
        assert_eq!(extract_tokens("token:5"), Some(5));
        assert_eq!(extract_tokens("Tokens:   1000  used"), Some(1000));
        assert_eq!(extract_tokens("notokens: 5"), None);
        assert_eq!(extract_tokens("tokens 5"), None); // 콜론 없음
        assert_eq!(extract_tokens("xtoken: 5"), None); // 워드 경계 위반
    }

    /// 환경 값은 인자로 전달해 실제 실행 환경과 관계없이 검사한다.
    #[test]
    fn resolve_surface_id_missing_and_no_env_is_invalid_params() {
        let err = resolve_surface_id_from(&json!({}), None, &test_translator()).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn resolve_surface_id_falls_back_to_env_when_the_param_is_absent() {
        let sid = resolve_surface_id_from(&json!({}), Some("42"), &test_translator()).unwrap();
        assert_eq!(sid, 42);
    }

    /// u32 범위를 넘는 값은 다른 id로 자르거나 환경 값으로 대신하지 않고 거절한다.
    #[test]
    fn a_surface_beyond_u32_is_rejected_not_truncated() {
        let big = serde_json::json!({ "surface": 5_000_000_000u64 });
        let err = resolve_surface_id_from(&big, Some("7"), &test_translator())
            .expect_err("u32 를 넘는 값은 오류여야 한다");
        assert_eq!(err.code, -32602);
        // 범위 안의 값은 그대로 받아야 한다.
        let ok = resolve_surface_id_from(
            &serde_json::json!({ "surface": 4_294_967_295u64 }),
            None,
            &test_translator(),
        )
        .expect("u32::MAX 는 유효한 값이다");
        assert_eq!(ok, u32::MAX);
    }

    /// 형식이 잘못된 값은 환경 값으로 대신하지 않고 거절한다.
    #[test]
    fn a_non_numeric_surface_is_rejected_and_does_not_fall_back_to_env() {
        for bad in [
            json!({ "surface": "conductor" }),
            json!({ "surface_id": "7x" }),
        ] {
            let err = resolve_surface_id_from(&bad, Some("42"), &test_translator())
                .expect_err("숫자가 아닌 surface 는 거부해야 한다");
            assert_eq!(err.code, -32602, "params={bad}");
        }
    }

    /// 세 언어 모두 잘못된 surface 인자 이름을 오류에 포함해야 한다.
    #[test]
    fn a_malformed_surface_in_a_hook_names_which_of_the_two_keys_was_wrong() {
        for locale in ["en", "ko", "ja"] {
            let tr = test_translator_for(locale);
            // env 는 채워둔다 — 폴백으로 넘어가면 문구 자체가 안 나온다.
            let by_surface = resolve_surface_id_from(&json!({ "surface": "x" }), Some("42"), &tr)
                .expect_err("문자열은 surface id 가 아니다")
                .message;
            let by_surface_id =
                resolve_surface_id_from(&json!({ "surface_id": "x" }), Some("42"), &tr)
                    .expect_err("문자열은 surface id 가 아니다")
                    .message;
            assert!(
                by_surface_id.contains("surface_id"),
                "{locale}: 'surface_id' 가 틀렸는데 그 이름을 안 댄다 — {by_surface_id}"
            );
            assert!(
                !by_surface.contains("surface_id"),
                "{locale}: 'surface' 가 틀렸는데 'surface_id' 를 댄다 — {by_surface}"
            );
            assert_ne!(
                by_surface, by_surface_id,
                "{locale}: 두 키가 같은 문구를 받는다 — 어느 쪽이 틀렸는지 못 가른다"
            );
        }
    }

    /// 두 대상 키가 서로 다르면 훅도 거절한다.
    #[test]
    fn two_names_disagreeing_is_refused_in_a_hook_too() {
        let err = resolve_surface_id_from(
            &json!({ "surface": 1, "surface_id": 2 }),
            Some("42"),
            &test_translator(),
        )
        .expect_err("두 이름이 다른 대상을 가리키면 고를 수 없다");
        assert_eq!(err.code, -32602);
        assert!(
            err.message.contains('1') && err.message.contains('2'),
            "충돌한 두 대상 값을 오류에 포함해야 한다: {}",
            err.message
        );
    }

    // 새 턴의 오류 중복 알림 초기화.

    #[test]
    fn is_new_turn_event_matches_active_transition_only() {
        for event in ["prompt-submit", "session-start", "active"] {
            assert!(is_new_turn_event(event), "{event} 은 새 턴 신호여야 함");
        }
        for event in [
            "stop",
            "subagent-stop",
            "notification",
            "session-end",
            "pre-tool-use",
            "post-tool-use",
        ] {
            assert!(!is_new_turn_event(event), "{event} 은 새 턴 신호가 아님");
        }
    }

    #[test]
    fn reset_dedupe_if_enabled_clears_tracked_surface() {
        let scanner = Arc::new(Mutex::new(ErrorScanner::new()));
        scanner
            .lock()
            .unwrap()
            .enable(7, crate::error_scan::ScanTarget::TopLevel);
        scanner
            .lock()
            .unwrap()
            .seed_dedupe_for_test(7, "API Error: boom");

        reset_dedupe_if_enabled(&scanner, 7);

        assert!(
            !scanner.lock().unwrap().has_dedupe_state(7),
            "추적 중인 surface 는 dedupe 가 초기화돼야 함"
        );
    }

    #[test]
    fn reset_dedupe_if_enabled_skips_untracked_surface() {
        // 감시 대상이 아니면 남아 있는 중복 기록도 건드리지 않아야 한다.
        let scanner = Arc::new(Mutex::new(ErrorScanner::new()));
        scanner
            .lock()
            .unwrap()
            .seed_dedupe_for_test(99, "unrelated");

        reset_dedupe_if_enabled(&scanner, 99);

        assert!(!scanner.lock().unwrap().is_enabled(99));
        assert!(
            scanner.lock().unwrap().has_dedupe_state(99),
            "추적 대상이 아니면 dedupe 상태를 건드리지 않아야 함"
        );
    }
}
