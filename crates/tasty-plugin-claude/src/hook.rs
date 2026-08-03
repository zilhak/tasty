//! `claude.hook` IPC 핸들러.
//!
//! CLI에서 `tasty claude hook <event> [--surface <id>] [--session <s>]`로 호출되며,
//! event별로 자식 상태(idle/needs_input/active)와 session meta, 텔레메트리를 산출한다.
//!
//! **idle/needs_input 신호는 자체 state 대신 호스트 registry(`terminal.set_state`)로
//! 주입한다**(occupancy-05). caller(spawn/tell 호출 주체)에게 완료를 알리는 건
//! `handlers.rs`의 1회성 알림 훅(`register_notify_hooks`/`claude.notify_done`)이
//! 이 모듈이 fire 하는 `claude-idle`/`needs-input` 이벤트를 구독해 처리한다 — 이
//! 모듈은 parent 관계를 조회하거나 fan-out 하지 않는다. wall-time 텔레메트리
//! 타이밍만 plugin 이 보유한다(`ClaudeState`).
//!
//! state 변이/host side effect 계산을 [`apply_hook`]으로 분리해 단위 테스트에서는
//! host 호출을 모킹하지 않고도 분기 로직을 검증할 수 있게 했다.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::error_scan::ErrorScanner;
use crate::state::ClaudeState;

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
    /// `surface.completion { surface_id }` — surface highlight(주의 환기) 발동.
    /// "Claude가 명령을 끝냈다"/"확인이 필요하다" 신호를
    /// `docs/features/surface-highlight/index.md` 의 producer 중립 highlight API 로
    /// 연결한다.
    SurfaceCompletion { surface_id: u32 },
}

pub fn handle_claude_hook(
    state: &mut ClaudeState,
    scanner: &Arc<Mutex<ErrorScanner>>,
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

    let mut calls = apply_hook(event, surface_id, session.as_deref())?;
    calls.extend(telemetry_for_hook(
        state, event, surface_id, message, now_ms,
    ));

    for call in &calls {
        deliver(host, call);
    }

    if is_new_turn_event(event) {
        reset_dedupe_if_enabled(scanner, surface_id);
    }

    Ok(json!({ "ok": true, "surface_id": surface_id, "event": event }))
}

/// 새 턴 시작(=idle 상태 해제) 신호 — `apply_hook` 이 `SetState{state:"active"}` 로
/// 묶는 이벤트 집합과 동일하다. 이 시점에 error dedupe 를 초기화해, 지난 턴의 에러
/// 텍스트로 눌려있던 dedupe 가 이번 턴에 나는 새 에러까지 억제하지 않게 한다.
fn is_new_turn_event(event: &str) -> bool {
    matches!(event, "prompt-submit" | "session-start" | "active")
}

/// scanner 가 이 surface 를 추적 중일 때만 dedupe 를 초기화한다 — error_scan 은
/// `handle_launch` 가 등록한 surface 만 enable 하므로(`handle_spawn` 자식 등은
/// 추적 대상이 아님), 추적하지 않는 surface 의 hook 이벤트는 조용히 건너뛴다.
fn reset_dedupe_if_enabled(scanner: &Arc<Mutex<ErrorScanner>>, surface_id: u32) {
    if let Ok(mut s) = scanner.lock()
        && s.is_enabled(surface_id)
    {
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

/// hook event → telemetry HostCall 매핑. 순수 함수 — 테스트가 직접 검증한다.
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

/// event → 자식 상태 갱신(`SetState`) + session meta 계산. host 에 의존하지 않는
/// 순수 함수 — 단위 테스트가 분기 동작을 직접 검증한다. 자식 상태
/// (idle/needs_input/active)는 호스트 registry 로 주입할 `SetState` HostCall 로
/// 표현한다(자체 state 미보유).
pub fn apply_hook(
    event: &str,
    surface_id: u32,
    session: Option<&str>,
) -> Result<Vec<HostCall>, IpcMethodError> {
    let mut calls = Vec::new();
    match event {
        "stop" | "subagent-stop" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion { surface_id });
        }
        "session-end" => {
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
                key: "restore.command",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion { surface_id });
        }
        "notification" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "needs_input",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "needs-input",
            });
            calls.push(HostCall::SurfaceCompletion { surface_id });
        }
        "prompt-submit" | "session-start" | "active" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "active",
            });
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
        HostCall::SurfaceCompletion { surface_id } => {
            ("surface.completion", json!({ "surface_id": surface_id }))
        }
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
        let calls = apply_hook("stop", 100, None).unwrap();
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
                HostCall::SurfaceCompletion { surface_id: 100 },
            ]
        );
    }

    #[test]
    fn subagent_stop_treated_like_stop() {
        let calls = apply_hook("subagent-stop", 7, None).unwrap();
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
                HostCall::SurfaceCompletion { surface_id: 7 },
            ]
        );
    }

    #[test]
    fn notification_sets_needs_input_and_fires_needs_input() {
        let calls = apply_hook("notification", 100, None).unwrap();
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
                HostCall::SurfaceCompletion { surface_id: 100 },
            ]
        );
    }

    #[test]
    fn session_end_clears_session_meta_and_fires_idle() {
        let calls = apply_hook("session-end", 100, None).unwrap();
        assert_eq!(
            calls,
            vec![
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
                HostCall::SurfaceCompletion { surface_id: 100 },
            ]
        );
    }

    // ── surface highlight 배선(`docs/features/surface-highlight/index.md`) —
    // stop/subagent-stop/session-end/notification 각각에서 SurfaceCompletion 이
    // 나가는지 개별 확인 — 위 4개 테스트가 이미 전체 시퀀스에 포함됨을 검증하지만,
    // 여기서는 "완료/확인필요 신호에는 항상 highlight 가 딸려온다"는 계약 자체를
    // 명시적으로 이름 붙여 pin 한다 ──

    #[test]
    fn stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("stop", 100, None).unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { surface_id: 100 }))
        );
    }

    #[test]
    fn subagent_stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("subagent-stop", 7, None).unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { surface_id: 7 }))
        );
    }

    #[test]
    fn session_end_also_raises_surface_completion_highlight() {
        let calls = apply_hook("session-end", 100, None).unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { surface_id: 100 }))
        );
    }

    #[test]
    fn notification_also_raises_surface_completion_highlight() {
        let calls = apply_hook("notification", 100, None).unwrap();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { surface_id: 100 }))
        );
    }

    #[test]
    fn prompt_submit_does_not_raise_surface_completion_highlight() {
        // "작업 시작" 신호는 완료/확인필요가 아니므로 highlight 대상이 아니다 —
        // apply_hook의 "prompt-submit"|"session-start"|"active" 분기는 건드리지 않는다
        // (`docs/features/surface-highlight/index.md` 참고).
        let calls = apply_hook("prompt-submit", 100, None).unwrap();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { .. }))
        );
    }

    #[test]
    fn prompt_submit_sets_active_and_no_meta() {
        let calls = apply_hook("prompt-submit", 100, None).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_start_without_session_id_just_sets_active() {
        let calls = apply_hook("session-start", 100, None).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_start_with_session_id_emits_meta_set() {
        let calls = apply_hook("session-start", 100, Some("sess-abc")).unwrap();
        assert_eq!(
            calls,
            vec![
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

    #[test]
    fn unknown_event_returns_invalid_params() {
        let err = apply_hook("bogus", 100, None).unwrap_err();
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
        if std::env::var("TASTY_SURFACE_ID").is_ok() {
            return;
        }
        let err = resolve_surface_id(&json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    // ── error dedupe 초기화 배선 (disable/reset_dedupe/is_enabled 재배선) ──

    #[test]
    fn is_new_turn_event_matches_active_transition_only() {
        for event in ["prompt-submit", "session-start", "active"] {
            assert!(is_new_turn_event(event), "{event} 은 새 턴 신호여야 함");
        }
        for event in ["stop", "subagent-stop", "notification", "session-end"] {
            assert!(!is_new_turn_event(event), "{event} 은 새 턴 신호가 아님");
        }
    }

    #[test]
    fn reset_dedupe_if_enabled_clears_tracked_surface() {
        let scanner = Arc::new(Mutex::new(ErrorScanner::new()));
        scanner.lock().unwrap().enable(7);
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
        // handle_spawn 자식처럼 error_scan 이 애초에 enable 하지 않은 surface — hook
        // 이벤트가 와도 조용히 무시해야 한다. dedupe 상태가 (다른 경로로) 이미
        // 있더라도 is_enabled 가 false 면 건드리지 않아야 함을 직접 확인한다.
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
