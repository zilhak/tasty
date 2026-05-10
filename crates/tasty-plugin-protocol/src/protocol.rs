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
/// host → plugin: plugin이 보낸 ipc.call에 대한 결과.
/// params에 [`IpcCallResult`].
pub const METHOD_IPC_RESULT: &str = "ipc.result";

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

/// 호스트 → plugin 요청.
#[derive(Debug, Clone, Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn response_with_result() {
        let s = r#"{"id":7,"result":{"ok":true}}"#;
        let parsed: PluginResponse = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.id, 7);
        assert!(parsed.result.is_some());
        assert!(parsed.error.is_none());
    }
}
