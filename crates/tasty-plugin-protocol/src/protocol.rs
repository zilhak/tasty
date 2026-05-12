//! Plugin ↔ Host 메시지 정의.
//!
//! Plugin이 connection 직후 첫 줄로 보내는 `AuthMessage`로 token 인증.
//! 이후 한 줄당 하나의 JSON 메시지(NDJSON 스타일).
//!
//! - 응답: `{"id": N, "result": ...}` 또는 `{"id": N, "error": "..."}`
//! - 알림 (id 없음): `{"event": {"kind": "...", ...}}`

use serde::{Deserialize, Serialize};

use crate::ui_tree::{UiEvent, UiNode};

// ── Host → plugin method names ──
pub const METHOD_PING: &str = "ping";
pub const METHOD_SHUTDOWN: &str = "shutdown";
pub const METHOD_HOST_HELLO: &str = "host.hello";
pub const METHOD_SURFACE_CREATE: &str = "surface.create";
pub const METHOD_SURFACE_EVENT: &str = "surface.event";
pub const METHOD_SURFACE_SNAPSHOT: &str = "surface.snapshot";
pub const METHOD_SURFACE_RESTORE: &str = "surface.restore";
pub const METHOD_SURFACE_DESTROY: &str = "surface.destroy";
/// host → plugin: lifecycle 통지 (다구독 broadcast). 매니페스트에 surface_observer로
/// 구독한 plugin들이 받는다. params에 [`SurfaceLifecycleParams`].
/// 응답은 fire-and-forget — 호스트가 무시한다.
pub const METHOD_SURFACE_LIFECYCLE: &str = "surface.lifecycle";
/// host → plugin: plugin이 보낸 ipc.call에 대한 결과.
/// params에 [`IpcCallResult`].
pub const METHOD_IPC_RESULT: &str = "ipc.result";
/// host → plugin: 사용자 단축키 매칭으로 plugin command가 트리거됨.
/// params에 [`CommandInvokeParams`]. plugin은 그에 따라 surface state를 변경하고,
/// 변경 결과는 `surface.event`와 동일하게 `SurfaceResult` 형태로 응답한다 (tree
/// 또는 display_name 갱신).
pub const METHOD_COMMAND_INVOKE: &str = "command.invoke";

/// `surface.create` / `surface.event` / `surface.restore` 응답에 포함되는 standard 결과.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceResult {
    /// 새로 그릴 트리. `None`이면 호스트는 이전 트리를 그대로 사용 (변경 없음).
    #[serde(default)]
    pub tree: Option<UiNode>,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// `surface.event` params — 호스트가 plugin에 보낼 사용자 이벤트.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceEventParams<'a> {
    pub surface_id: u32,
    pub event: &'a UiEvent,
}

/// `command.invoke` params — 사용자 단축키 매칭 시 호스트가 plugin에 보내는 명령.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandInvokeParams {
    pub surface_id: u32,
    pub command_id: String,
}

/// `surface.lifecycle` params — 호스트가 구독 plugin들에게 broadcast하는 lifecycle 통지.
///
/// `surface.destroy`(owner plugin 한정)와 달리 자기 소유가 아닌 surface의 lifecycle도
/// 알리기 위한 메서드. 매니페스트의 `[[contributes.surface_observer]]`로 명시적
/// opt-in한 plugin만 받는다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceLifecycleParams {
    pub event: SurfaceLifecycleEvent,
    pub surface_id: u32,
    /// surface kind 문자열 (예: "terminal", "explorer"). plugin이 빠르게 필터링하기 위함.
    pub kind: String,
    pub reason: SurfaceCloseReason,
}

/// `surface.lifecycle`의 event 종류. 현재 `closed`만 정의. 향후 `before_close`/`created`
/// 등이 추가될 수 있으나, 호환성을 위해 plugin은 알 수 없는 값을 무시해야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycleEvent {
    Closed,
}

/// surface 닫힘 원인. 호출 컨텍스트에서 호스트가 부여한다.
///
/// - `UserClose`: 사용자 단축키/마우스로 닫음 (예: pane 닫기 단축키, 탭 우클릭 닫기)
/// - `AgentClose`: IPC/CLI를 통해 에이전트가 명시적으로 닫음 (예: `tasty close surface`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceCloseReason {
    UserClose,
    AgentClose,
}

/// 호스트 → plugin 요청.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginRequest {
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

/// plugin → 호스트 응답.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
    /// JSON-RPC 에러 코드. 없으면 host가 -32000 (server error)으로 간주.
    /// 예: -32601 method not found, -32602 invalid params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<i32>,
}

/// plugin → 호스트 비동기 알림 (요청 응답이 아님).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginEvent {
    /// 매니페스트 검증 후 호스트가 받는 첫 메시지.
    Hello {
        plugin_id: String,
        version: String,
    },
    /// surface invalidated — 호스트가 다음 프레임에 redraw (단계 06).
    SurfaceInvalidated { surface_id: u32 },
    /// host action 트리거 (단계 06).
    NotifyHost {
        surface_id: u32,
        event: String,
        payload: serde_json::Value,
    },
    /// plugin 측 로그 (호스트 로그에 합쳐짐).
    Log { level: String, message: String },
    /// plugin → 호스트 IPC 호출. 호스트가 권한을 검사하고 라우터에 보낸 뒤,
    /// 결과를 `ipc.result` 요청으로 회신한다 (`call_id`로 매칭).
    IpcCall {
        call_id: u64,
        method: String,
        params: serde_json::Value,
    },
}

/// `ipc.result` 요청의 params — plugin의 ipc.call에 대한 호스트의 응답.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IpcCallResult {
    pub call_id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// plugin이 connection 직후 첫 줄로 보내는 인증 메시지.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthMessage {
    pub plugin_id: String,
    pub token: String,
}

/// 호스트 → plugin 인증 단계 전용 ack. plugin이 [`AuthMessage`]를 보낸 뒤
/// 메인 메시지 루프에 진입하기 전, **단 한 번** 같은 NDJSON 채널로 수신한다.
///
/// `ok=false`이면 plugin SDK가 [`crate::PluginError::HandshakeRejected`](
/// 같은 이름의 SDK variant)로 즉시 실패한다. 호스트 측은
/// `src/plugin/listener.rs`에서 토큰 검증 결과에 따라 송신한다.
///
/// envelope: `{"auth_ack": { "ok": true }}` 또는 `{"auth_ack": { "ok": false, "reason": "..." }}`.
/// 메인 루프의 `PluginRequest`와 다른 envelope를 사용해 파서 분리.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthAck {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// AuthAck의 envelope wrapper. NDJSON 한 줄에 담기는 최상위 구조.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthAckEnvelope {
    pub auth_ack: AuthAck,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn auth_round_trip() {
        let msg = AuthMessage {
            plugin_id: "com.example.x".into(),
            token: "abc123".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let parsed: AuthMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.plugin_id, "com.example.x");
        assert_eq!(parsed.token, "abc123");
    }

    #[test]
    fn event_hello_round_trip() {
        let ev = PluginEvent::Hello {
            plugin_id: "com.example.x".into(),
            version: "0.1.0".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"hello\""));
        let parsed: PluginEvent = serde_json::from_str(&s).unwrap();
        match parsed {
            PluginEvent::Hello { plugin_id, version } => {
                assert_eq!(plugin_id, "com.example.x");
                assert_eq!(version, "0.1.0");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn response_with_error() {
        let s = r#"{"id":42,"error":"boom"}"#;
        let parsed: PluginResponse = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.error.as_deref(), Some("boom"));
        assert!(parsed.result.is_none());
        assert!(parsed.error_code.is_none());
    }

    #[test]
    fn response_with_result() {
        let s = r#"{"id":7,"result":{"ok":true}}"#;
        let parsed: PluginResponse = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.id, 7);
        assert!(parsed.result.is_some());
        assert!(parsed.error.is_none());
        assert!(parsed.error_code.is_none());
    }

    #[test]
    fn response_with_error_code() {
        let resp = PluginResponse {
            id: 1,
            result: None,
            error: Some("method not found".into()),
            error_code: Some(-32601),
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"error_code\":-32601"));
        let parsed: PluginResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.error_code, Some(-32601));
    }

    #[test]
    fn auth_ack_envelope_round_trip() {
        let env = AuthAckEnvelope {
            auth_ack: AuthAck {
                ok: false,
                reason: Some("token mismatch".into()),
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"auth_ack\""));
        assert!(s.contains("\"ok\":false"));
        let parsed: AuthAckEnvelope = serde_json::from_str(&s).unwrap();
        assert!(!parsed.auth_ack.ok);
        assert_eq!(parsed.auth_ack.reason.as_deref(), Some("token mismatch"));
    }

    #[test]
    fn auth_ack_omits_reason_when_ok() {
        let env = AuthAckEnvelope {
            auth_ack: AuthAck {
                ok: true,
                reason: None,
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(!s.contains("reason"));
    }

    #[test]
    fn response_omits_error_code_when_none() {
        let resp = PluginResponse {
            id: 1,
            result: Some(Value::Null),
            error: None,
            error_code: None,
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(!s.contains("error_code"));
    }

    #[test]
    fn surface_lifecycle_params_round_trip() {
        let params = SurfaceLifecycleParams {
            event: SurfaceLifecycleEvent::Closed,
            surface_id: 42,
            kind: "terminal".into(),
            reason: SurfaceCloseReason::UserClose,
        };
        let s = serde_json::to_string(&params).unwrap();
        assert!(s.contains("\"event\":\"closed\""));
        assert!(s.contains("\"reason\":\"user_close\""));
        let parsed: SurfaceLifecycleParams = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.event, SurfaceLifecycleEvent::Closed);
        assert_eq!(parsed.surface_id, 42);
        assert_eq!(parsed.kind, "terminal");
        assert_eq!(parsed.reason, SurfaceCloseReason::UserClose);
    }

    #[test]
    fn surface_close_reason_agent_serializes_snake_case() {
        let params = SurfaceLifecycleParams {
            event: SurfaceLifecycleEvent::Closed,
            surface_id: 7,
            kind: "explorer".into(),
            reason: SurfaceCloseReason::AgentClose,
        };
        let s = serde_json::to_string(&params).unwrap();
        assert!(s.contains("\"reason\":\"agent_close\""));
        let parsed: SurfaceLifecycleParams = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.reason, SurfaceCloseReason::AgentClose);
    }
}
