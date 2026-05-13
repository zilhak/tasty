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

use serde_json::{json, Value};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::state::ClaudeState;

/// hook 처리 후 plugin이 호스트에 보낼 IPC 호출 1건.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCall {
    /// `surface.fire_hook { surface_id, event }`
    FireHook { surface_id: u32, event: &'static str },
    /// `surface.meta.set { surface_id, key, value }`
    MetaSet {
        surface_id: u32,
        key: &'static str,
        value: String,
    },
    /// `surface.meta.unset { surface_id, key }`
    MetaUnset {
        surface_id: u32,
        key: &'static str,
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
    let session = params.get("session").and_then(|v| v.as_str()).map(String::from);

    let calls = apply_hook(state, event, surface_id, session.as_deref())?;
    state.save();

    for call in calls {
        deliver(host, &call);
    }
    Ok(json!({ "ok": true, "surface_id": surface_id, "event": event }))
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
            if event == "session-start" {
                if let Some(session_id) = session {
                    calls.push(HostCall::MetaSet {
                        surface_id,
                        key: "claude-session-id",
                        value: session_id.to_string(),
                    });
                }
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
    if let Ok(env) = std::env::var("TASTY_SURFACE_ID") {
        if let Ok(sid) = env.parse::<u32>() {
            return Ok(sid);
        }
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
            [HostCall::FireHook { surface_id: 7, event: "claude-idle" }]
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
            vec![HostCall::MetaSet {
                surface_id: 100,
                key: "claude-session-id",
                value: "sess-abc".into(),
            }]
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
        assert_eq!(
            resolve_surface_id(&json!({ "surface": 42 })).unwrap(),
            42
        );
        assert_eq!(
            resolve_surface_id(&json!({ "surface_id": 7 })).unwrap(),
            7
        );
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
