//! 호스트와 플러그인이 주고받는 이벤트의 공통 구조.

use serde::{Deserialize, Serialize};

/// 이벤트 재발행의 최대 단계. 호스트에서 시작할 때 hop은 0이며,
/// 플러그인이 받은 이벤트를 다시 발행하면 증가한다. 한도를 넘으면 호스트가 거절한다.
pub const MAX_HOP: u8 = 16;

/// 한 줄에 담기는 이벤트 메시지의 최상위 구조.
///
/// `key`는 `<namespace>.<event_name>` 포맷이며, 예약 네임스페이스
/// (catalog 문서 참조)에는 호스트만 publish할 수 있다.
///
/// `payload`는 카탈로그에서 정의한 페이로드 Rust 타입을 `serde_json::Value`로
/// 직렬화한 것이다. 자세한 스키마는 [`super::payloads`] 모듈 참조.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventEnvelope {
    pub key: String,
    pub payload: serde_json::Value,
    pub meta: EventMeta,
}

/// 모든 이벤트가 공유하는 메타데이터. trace_id는 한 이벤트에서 이어진
/// 재발행을 같은 흐름으로 찾아볼 수 있도록 유지한다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventMeta {
    pub trace_id: String,
    pub hop: u8,
    pub origin: EventOrigin,
    pub scope: EventScope,
}

/// 이벤트를 발행한 주체.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventOrigin {
    /// 호스트가 직접 발행했다. hop은 0이다.
    Host,
    /// 플러그인이 publish API로 발행했다.
    Plugin { plugin_id: String },
}

/// 이벤트가 특정 surface에 속하는지 구분한다.
/// Tab, Pane, Workspace 등의 구체적인 ID는 payload에 넣는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventScope {
    /// 특정 surface에 매이지 않는 전역 사건.
    /// 예: `theme.changed`, `plugin.loaded`, `workspace.activated`.
    System,
    /// 특정 surface 기준 사건. 대상 surface ID는 payload의 `surface_id` 등.
    /// 예: `surface.focused`, `process.exited`.
    Surface,
}

/// 닫힘 이벤트의 사유. 부모를 닫아 자식이 함께 닫히면 같은 사유를 사용한다.
/// 예를 들어 사용자가 창을 닫으면 그 안의 surface도 User로 기록한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleReason {
    /// 사용자가 단축키/마우스/UI 버튼으로 직접 닫음.
    User,
    /// CLI 또는 plugin의 IPC 호출로 닫음 (에이전트 자동화 포함).
    Ipc,
    /// 비정상 종료. PTY 프로세스 크래시, plugin 강제 종료 등.
    Crash,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_round_trip() {
        let env = EventEnvelope {
            key: "surface.focused".into(),
            payload: json!({ "surface_id": 42, "prev_surface_id": 7 }),
            meta: EventMeta {
                trace_id: "trace-abc".into(),
                hop: 0,
                origin: EventOrigin::Host,
                scope: EventScope::Surface,
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        let parsed: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.key, "surface.focused");
        assert_eq!(parsed.meta.hop, 0);
        assert_eq!(parsed.meta.scope, EventScope::Surface);
        assert_eq!(parsed.meta.origin, EventOrigin::Host);
    }

    #[test]
    fn origin_plugin_round_trip() {
        let o = EventOrigin::Plugin {
            plugin_id: "com.example.x".into(),
        };
        let s = serde_json::to_string(&o).unwrap();
        // tag style: {"kind":"plugin","plugin_id":"..."}
        assert!(s.contains("\"kind\":\"plugin\""));
        assert!(s.contains("\"plugin_id\":\"com.example.x\""));
        let parsed: EventOrigin = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, o);
    }

    #[test]
    fn scope_serializes_snake_case() {
        let s = serde_json::to_string(&EventScope::System).unwrap();
        assert_eq!(s, "\"system\"");
        let s = serde_json::to_string(&EventScope::Surface).unwrap();
        assert_eq!(s, "\"surface\"");
    }

    #[test]
    fn lifecycle_reason_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&LifecycleReason::User).unwrap(),
            "\"user\""
        );
        assert_eq!(
            serde_json::to_string(&LifecycleReason::Ipc).unwrap(),
            "\"ipc\""
        );
        assert_eq!(
            serde_json::to_string(&LifecycleReason::Crash).unwrap(),
            "\"crash\""
        );
    }
}
