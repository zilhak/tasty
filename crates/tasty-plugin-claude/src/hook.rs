//! `claude.hook` IPC 핸들러.
//!
//! 호스트 `src/cli/claude.rs::run_claude_hook`의 로직을 1:1 옮긴 것. CLI에서
//! `tasty claude hook <event> [--surface <id>] [--session <s>]`로 호출되며,
//! event별로 ClaudeState의 idle/needs_input을 갱신하고 호스트 IPC로
//! `surface.fire_hook` / `surface.meta.set` / `surface.meta.unset`를 호출한다.
//!
//! 호스트 측과 달리 plugin은 idle/needs_input을 자기 state에서 직접 다루므로
//! `claude.set_idle_state` / `claude.set_needs_input` IPC를 거치지 않는다.
//! cutover(step 04) 후엔 그 IPC 메서드들 자체가 사라진다.
//!
//! state 변이와 host 측 side effect 계산을 [`apply_hook`]으로 분리해 단위
//! 테스트에서는 host 호출을 모킹하지 않고도 분기 로직을 검증할 수 있게 했다.

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::state::ClaudeState;

/// hook 처리 후 plugin이 호스트에 보낼 IPC 호출 1건.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
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
    /// `telemetry.record { metric, value, op, workspace_id?, agent?, tags? }`
    ///
    /// Phase 4.6 — claude hook 이 자동 발행하는 메트릭. `agent` 는
    /// host 측에서 plugin id (`tasty.com.tasty.claude`) 로 자동 결정되므로
    /// 여기선 omit 한다. `surface_id` 는 tags 에 string 으로 담는다.
    TelemetryRecord {
        metric: &'static str,
        value: f64,
        surface_id: u32,
    },
}

pub fn handle_claude_hook(
    state: &mut ClaudeState,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'event'"))?;

    let surface_id = resolve_surface_id(params)?;
    let session = params
        .get("session")
        .and_then(|v| v.as_str())
        .map(String::from);
    let message = params.get("message").and_then(|v| v.as_str());
    let now_ms = now_ms();

    let mut calls = apply_hook(state, event, surface_id, session.as_deref())?;
    calls.extend(telemetry_for_hook(
        state, event, surface_id, message, now_ms,
    ));
    state.save();

    for call in calls {
        deliver(host, &call);
    }
    Ok(json!({ "ok": true, "surface_id": surface_id, "event": event }))
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// hook event → telemetry HostCall 매핑. 순수 함수 (host 미사용) — 테스트가
/// 직접 검증한다.
///
/// - `session-start` → state 에 시작 시각 기록 (HostCall 없음)
/// - `stop` / `subagent-stop` / `session-end` → wall_time_ms 가 있으면 발행
/// - `notification` → message 에서 `\btokens?:\s*(\d+)\b` 매칭되면 input_tokens 발행
pub fn telemetry_for_hook(
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
        "stop" | "subagent-stop" | "session-end" => {
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

/// `\btokens?:\s*(\d+)\b` 휴리스틱. 정규식 dep 추가를 피하려고 수동 스캔.
fn extract_tokens(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // word boundary: i==0 또는 이전 char 가 alnum 아니면 통과
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

/// state를 변이하고 호스트에 보낼 IPC 호출 목록을 반환. host에 의존하지 않으므로
/// 단위 테스트가 분기 동작을 직접 검증할 수 있다.
pub fn apply_hook(
    state: &mut ClaudeState,
    event: &str,
    surface_id: u32,
    session: Option<&str>,
) -> Result<Vec<HostCall>, IpcMethodError> {
    let mut calls = Vec::new();
    match event {
        "stop" | "subagent-stop" => {
            state.set_idle(surface_id, true);
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
        }
        "session-end" => {
            state.set_idle(surface_id, true);
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: "claude-session-id",
            });
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: "restore.command",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
        }
        "notification" => {
            state.set_needs_input(surface_id, true);
            calls.push(HostCall::FireHook {
                surface_id,
                event: "needs-input",
            });
        }
        "prompt-submit" | "session-start" | "active" => {
            // set_idle(false)는 needs_input도 함께 clear (state invariant).
            state.set_idle(surface_id, false);
            if event == "session-start"
                && let Some(session_id) = session
            {
                calls.push(HostCall::MetaSet {
                    surface_id,
                    key: "claude-session-id",
                    value: session_id.to_string(),
                });
                calls.push(HostCall::MetaSet {
                    surface_id,
                    key: "restore.command",
                    value: format!("claude -r {session_id}"),
                });
            }
        }
        other => {
            return Err(IpcMethodError::invalid_params(&format!(
                "unknown hook event '{other}' (expected: stop|subagent-stop|notification|session-end|prompt-submit|session-start|active)"
            )));
        }
    }
    Ok(calls)
}

fn deliver(host: &HostHandle, call: &HostCall) {
    let (method, params) = match call {
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
    };
    if let Err(e) = host.call(method, params) {
        tracing::warn!("claude hook host call '{method}' failed: {e}");
    }
}

fn resolve_surface_id(params: &Value) -> Result<u32, IpcMethodError> {
    if let Some(sid) = params
        .get("surface")
        .and_then(|v| v.as_u64())
        .or_else(|| params.get("surface_id").and_then(|v| v.as_u64()))
    {
        return Ok(sid as u32);
    }
    if let Ok(env) = std::env::var("TASTY_SURFACE_ID")
        && let Ok(sid) = env.parse::<u32>()
    {
        return Ok(sid);
    }
    Err(IpcMethodError::invalid_params(
        "no surface id (pass --surface or set TASTY_SURFACE_ID)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_sets_idle_and_emits_fire_hook() {
        let mut state = ClaudeState::default();
        let calls = apply_hook(&mut state, "stop", 100, None).unwrap();
        assert_eq!(state.state_of(100), "idle");
        assert_eq!(
            calls,
            vec![HostCall::FireHook {
                surface_id: 100,
                event: "claude-idle",
            }]
        );
    }

    #[test]
    fn subagent_stop_treated_like_stop() {
        let mut state = ClaudeState::default();
        let calls = apply_hook(&mut state, "subagent-stop", 7, None).unwrap();
        assert_eq!(state.state_of(7), "idle");
        assert!(matches!(
            calls.as_slice(),
            [HostCall::FireHook {
                surface_id: 7,
                event: "claude-idle"
            }]
        ));
    }

    #[test]
    fn notification_sets_needs_input_and_fires_needs_input() {
        let mut state = ClaudeState::default();
        let calls = apply_hook(&mut state, "notification", 100, None).unwrap();
        assert_eq!(state.state_of(100), "needs_input");
        assert_eq!(
            calls,
            vec![HostCall::FireHook {
                surface_id: 100,
                event: "needs-input",
            }]
        );
    }

    #[test]
    fn session_end_clears_session_meta_and_fires_idle() {
        let mut state = ClaudeState::default();
        let calls = apply_hook(&mut state, "session-end", 100, None).unwrap();
        assert_eq!(state.state_of(100), "idle");
        assert_eq!(
            calls,
            vec![
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
            ]
        );
    }

    #[test]
    fn prompt_submit_clears_idle_and_needs_input() {
        let mut state = ClaudeState::default();
        state.set_idle(100, true);
        state.set_needs_input(100, true);
        let calls = apply_hook(&mut state, "prompt-submit", 100, None).unwrap();
        assert_eq!(state.state_of(100), "active");
        assert!(calls.is_empty(), "prompt-submit emits no host calls");
    }

    #[test]
    fn session_start_without_session_id_just_clears() {
        let mut state = ClaudeState::default();
        state.set_idle(100, true);
        let calls = apply_hook(&mut state, "session-start", 100, None).unwrap();
        assert_eq!(state.state_of(100), "active");
        assert!(calls.is_empty());
    }

    #[test]
    fn session_start_with_session_id_emits_meta_set() {
        let mut state = ClaudeState::default();
        let calls = apply_hook(&mut state, "session-start", 100, Some("sess-abc")).unwrap();
        assert_eq!(state.state_of(100), "active");
        assert_eq!(
            calls,
            vec![
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

    #[test]
    fn unknown_event_returns_invalid_params() {
        let mut state = ClaudeState::default();
        let err = apply_hook(&mut state, "bogus", 100, None).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("bogus"));
    }

    #[test]
    fn resolve_surface_id_prefers_explicit_param() {
        assert_eq!(resolve_surface_id(&json!({ "surface": 42 })).unwrap(), 42);
        assert_eq!(resolve_surface_id(&json!({ "surface_id": 7 })).unwrap(), 7);
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

    #[test]
    fn resolve_surface_id_missing_returns_invalid_params() {
        // env에 TASTY_SURFACE_ID가 우연히 있으면 본 테스트가 신뢰성을 잃는다.
        // 위 양수 테스트들과 달리 본 테스트는 env에 의존하므로, 부재 케이스를
        // 보장할 수 없는 환경에서는 의미가 없다. 따라서 env가 있으면 스킵.
        if std::env::var("TASTY_SURFACE_ID").is_ok() {
            return;
        }
        let err = resolve_surface_id(&json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
    }
}
