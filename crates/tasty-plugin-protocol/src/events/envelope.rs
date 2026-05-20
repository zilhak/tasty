//! Event Bus envelope — 호스트와 plugin 사이를 흐르는 사건의 공통 wrapper.

use serde::{Deserialize, Serialize};

/// 한 사건의 chain이 거칠 수 있는 최대 hop 단계.
///
/// 호스트 발화 시 hop=0. plugin이 콜백 안에서 다시 publish하면 hop+1로 발화된다.
/// `hop > MAX_HOP`이면 dispatcher가 차단하고 warn 로그를 남긴다. 의도된 사용
/// 패턴에서 hop이 2~3을 넘기기 어렵다 — 이 상수는 무한 루프 방지 장치다.
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

/// 이벤트 envelope의 메타데이터. payload와 무관하게 모든 이벤트가 공유하는 정보.
///
/// `trace_id`는 한 사건이 만들어낸 후속 chain 전체에 공유되는 opaque 식별자다.
/// 호스트 발화 시점에 생성되고 plugin이 그 이벤트를 받아 재발화하면 동일한
/// `trace_id`가 그대로 전파된다. 디버깅·로그 상관관계 분석용.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventMeta {
    pub trace_id: String,
    pub hop: u8,
    pub origin: EventOrigin,
    pub scope: EventScope,
}

/// 누가 이 이벤트를 발화했는가.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventOrigin {
    /// 호스트 엔진이 직접 발화. hop=0.
    Host,
    /// 어떤 plugin이 publish API로 발화.
    Plugin { plugin_id: String },
}

/// 이벤트가 surface에 매여 있는지 여부.
///
/// 더 세분화된 scope(tab, pane, workspace 등)는 1.0에서 도입하지 않는다 —
/// plugin 입장에서 "surface와 관련 있는가"만 분기하면 charter 처리·필터·로그가
/// 단순해진다. 구체적인 ID(예: `surface_id`, `tab_id`)는 payload 필드로 전달한다.
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

/// 종료 계열 이벤트(`*.closed`, `plugin.unloaded` 등)의 공통 `reason` 값.
///
/// `parent_closed` 같은 cascade 분류는 별도 두지 않는다 — 부모를 닫은 주체가
/// 그대로 자식의 reason이 된다. (사용자가 윈도우를 닫으면 그 안의 모든 surface는
/// `User`, agent가 IPC로 workspace를 닫으면 그 안의 모든 surface는 `Ipc`.)
///
/// 더 세분화된 정보가 필요하면 1.x에서 옵션 필드 `reason_detail`을 추가한다.
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
