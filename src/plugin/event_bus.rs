//! Event Bus 1.0 — 호스트 ↔ plugin 간 브로드캐스트 이벤트 라우터.
//!
//! 책임:
//! - 매니페스트의 `event_subscribe`/`event_publish` 패턴을 권한 게이트로 보유
//! - plugin 또는 호스트가 발화한 [`EventEnvelope`]를 구독 패턴에 매칭되는 모든 대상에 fan-out
//! - 호스트 본문은 `publish()`로 직접 발화, plugin은 [`PluginEvent::EventPublish`] 경로로 위임
//! - hop count(`MAX_HOP=16`) 초과 envelope는 폐기하고 경고 로그
//! - 호스트 listener와 plugin listener를 통합된 [`Subscriber`] 인터페이스로 다룬다
//!
//! 패턴 매칭은 매니페스트 검증과 같은 형식을 사용한다:
//! - `surface.created` — 정확 일치
//! - `surface.*` — namespace 와일드카드 (마지막 세그먼트만 `*`)
//! - 매뉴얼 파싱이라 의존성 없음
//!
//! 권한 모델:
//! - plugin의 `event_subscribe` 패턴과 subscribe 요청 패턴이 매칭되어야 등록 허용
//! - plugin의 `event_publish` 패턴과 발화 envelope key가 매칭되어야 publish 허용
//! - 호스트 publish는 권한 검사 없이 항상 통과 (origin = Host)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use tasty_plugin_protocol::{
    EventDispatchParams, EventEnvelope, EventOrigin, METHOD_EVENT_DISPATCH, MAX_HOP,
    PluginRequest,
};

/// 패턴 매칭 헬퍼. 검증된 패턴은 정확 key 또는 `<segs>.*` 형태로 정규화돼 있다고 가정.
fn pattern_matches(pattern: &str, key: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(".*") {
        if let Some(rest) = key.strip_prefix(prefix) {
            // 와일드카드는 `<prefix>.<segment>` 형태에 일치. `prefix`와 정확히 같은 키는 거부.
            rest.starts_with('.') && rest.len() > 1
        } else {
            false
        }
    } else {
        pattern == key
    }
}

/// 호스트 측 구독자. 토픽이 매칭되면 mpsc로 envelope를 받아간다.
/// 호스트 본문이 자기 화면/스토어 갱신용으로 listen할 때 사용.
#[allow(dead_code)]
pub struct HostSubscription {
    sub_id: u64,
    pattern: String,
    tx: mpsc::Sender<EventEnvelope>,
}

#[allow(dead_code)]
struct PluginSubscription {
    plugin_id: String,
    sub_id: u64,
    pattern: String,
}

/// EventBus 내부 상태. 락 하나로 모든 구독 테이블을 보호.
struct Inner {
    next_sub_id: AtomicU64,
    host_subs: Vec<HostSubscription>,
    plugin_subs: Vec<PluginSubscription>,
    /// 매니페스트의 `event_subscribe` 패턴 (plugin_id → 패턴 목록).
    plugin_subscribe_perms: HashMap<String, Vec<String>>,
    /// 매니페스트의 `event_publish` 패턴 (plugin_id → 패턴 목록).
    plugin_publish_perms: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<Inner>>,
}

/// fan-out 결과. 호스트의 plugin 송신 루프가 후처리한다.
/// 락 안에서 `Vec<PluginRequest>`를 만들지 않고 (plugin_id, request_id, EventDispatchParams) 페어만 모아준다.
#[derive(Debug)]
pub struct PluginDispatch {
    pub plugin_id: String,
    pub sub_id: u64,
    pub envelope: EventEnvelope,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next_sub_id: AtomicU64::new(1),
                host_subs: Vec::new(),
                plugin_subs: Vec::new(),
                plugin_subscribe_perms: HashMap::new(),
                plugin_publish_perms: HashMap::new(),
            })),
        }
    }

    /// plugin이 호스트에 등록될 때 매니페스트의 권한을 적재한다. 비활성화/언인스톨 시 `clear_plugin`으로 정리.
    pub fn set_plugin_permissions(
        &self,
        plugin_id: &str,
        subscribe_patterns: Vec<String>,
        publish_patterns: Vec<String>,
    ) {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        inner
            .plugin_subscribe_perms
            .insert(plugin_id.to_string(), subscribe_patterns);
        inner
            .plugin_publish_perms
            .insert(plugin_id.to_string(), publish_patterns);
    }

    /// plugin이 종료되거나 비활성화되면 권한 + 구독 모두 제거.
    pub fn clear_plugin(&self, plugin_id: &str) {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        inner.plugin_subscribe_perms.remove(plugin_id);
        inner.plugin_publish_perms.remove(plugin_id);
        inner.plugin_subs.retain(|s| s.plugin_id != plugin_id);
    }

    /// 호스트 본문이 자기 listener를 등록할 때 사용. 반환된 `sub_id`로 `unsubscribe_host` 가능.
    pub fn subscribe_host(
        &self,
        pattern: impl Into<String>,
        tx: mpsc::Sender<EventEnvelope>,
    ) -> u64 {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        let sub_id = inner.next_sub_id.fetch_add(1, Ordering::Relaxed);
        inner.host_subs.push(HostSubscription {
            sub_id,
            pattern: pattern.into(),
            tx,
        });
        sub_id
    }

    #[allow(dead_code)]
    pub fn unsubscribe_host(&self, sub_id: u64) {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        inner.host_subs.retain(|s| s.sub_id != sub_id);
    }

    /// plugin이 `event.subscribe` IPC로 등록한 구독. 매니페스트 권한과 매칭되지 않으면 `Err`.
    /// 같은 `(plugin_id, sub_id)` 페어로 다시 호출되면 마지막 호출이 이긴다.
    pub fn subscribe_plugin(
        &self,
        plugin_id: &str,
        sub_id: u64,
        pattern: String,
    ) -> Result<(), EventBusError> {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        let allowed = inner
            .plugin_subscribe_perms
            .get(plugin_id)
            .map(|patterns| patterns.iter().any(|p| pattern_covers(p, &pattern)))
            .unwrap_or(false);
        if !allowed {
            return Err(EventBusError::SubscribeDenied {
                plugin_id: plugin_id.to_string(),
                pattern,
            });
        }
        // 중복 sub_id면 교체.
        inner
            .plugin_subs
            .retain(|s| !(s.plugin_id == plugin_id && s.sub_id == sub_id));
        inner.plugin_subs.push(PluginSubscription {
            plugin_id: plugin_id.to_string(),
            sub_id,
            pattern,
        });
        Ok(())
    }

    pub fn unsubscribe_plugin(&self, plugin_id: &str, sub_id: u64) {
        let mut inner = self.inner.lock().expect("event bus poisoned");
        inner
            .plugin_subs
            .retain(|s| !(s.plugin_id == plugin_id && s.sub_id == sub_id));
    }

    /// 호스트 본문이 새 envelope를 발화. 호스트는 모든 namespace에 publish 가능.
    /// 매칭되는 구독자에게 fan-out하고, plugin 측 dispatch 큐를 반환한다.
    pub fn publish_from_host(&self, envelope: EventEnvelope) -> Vec<PluginDispatch> {
        self.fan_out(envelope, None)
    }

    /// plugin이 발화한 envelope. publish 권한 매칭 + hop count 검사 후 fan-out.
    pub fn publish_from_plugin(
        &self,
        plugin_id: &str,
        envelope: EventEnvelope,
    ) -> Result<Vec<PluginDispatch>, EventBusError> {
        if envelope.meta.hop > MAX_HOP {
            return Err(EventBusError::HopExceeded {
                key: envelope.key,
                hop: envelope.meta.hop,
            });
        }
        // origin이 자기 자신을 가리키는지 확인 (Plugin { plugin_id } 일치).
        let origin_matches = match &envelope.meta.origin {
            EventOrigin::Plugin { plugin_id: pid } => pid == plugin_id,
            EventOrigin::Host => false,
        };
        if !origin_matches {
            return Err(EventBusError::OriginMismatch {
                plugin_id: plugin_id.to_string(),
                envelope_origin: envelope.meta.origin.clone(),
            });
        }
        // publish 권한 매칭.
        let allowed = {
            let inner = self.inner.lock().expect("event bus poisoned");
            inner
                .plugin_publish_perms
                .get(plugin_id)
                .map(|patterns| patterns.iter().any(|p| pattern_matches(p, &envelope.key)))
                .unwrap_or(false)
        };
        if !allowed {
            return Err(EventBusError::PublishDenied {
                plugin_id: plugin_id.to_string(),
                key: envelope.key,
            });
        }
        Ok(self.fan_out(envelope, Some(plugin_id)))
    }

    /// 실제 fan-out. 호스트 구독자에게는 그대로 보내고, plugin 구독자는 dispatch 페어로 모아 반환.
    /// 자기 자신이 발화한 이벤트는 자기 plugin 구독자에게 보내지 않는다 (loop 1차 방지).
    fn fan_out(
        &self,
        envelope: EventEnvelope,
        publisher_plugin_id: Option<&str>,
    ) -> Vec<PluginDispatch> {
        let inner = self.inner.lock().expect("event bus poisoned");
        // 호스트 구독자.
        let mut dead_host: Vec<u64> = Vec::new();
        for sub in &inner.host_subs {
            if pattern_matches(&sub.pattern, &envelope.key) {
                if sub.tx.send(envelope.clone()).is_err() {
                    dead_host.push(sub.sub_id);
                }
            }
        }
        // plugin 구독자.
        let mut dispatches: Vec<PluginDispatch> = Vec::new();
        for sub in &inner.plugin_subs {
            if Some(sub.plugin_id.as_str()) == publisher_plugin_id {
                continue;
            }
            if pattern_matches(&sub.pattern, &envelope.key) {
                dispatches.push(PluginDispatch {
                    plugin_id: sub.plugin_id.clone(),
                    sub_id: sub.sub_id,
                    envelope: envelope.clone(),
                });
            }
        }
        drop(inner);
        if !dead_host.is_empty() {
            let mut inner = self.inner.lock().expect("event bus poisoned");
            inner.host_subs.retain(|s| !dead_host.contains(&s.sub_id));
        }
        dispatches
    }

    /// 호스트가 plugin에 보낼 `event.dispatch` request param을 만든다. 송신은 호출 측이 담당.
    pub fn build_dispatch_request(dispatch: &PluginDispatch) -> PluginRequest {
        let params = EventDispatchParams {
            sub_id: dispatch.sub_id,
            envelope: dispatch.envelope.clone(),
        };
        PluginRequest {
            id: 0, // 호출 측이 채워 넣는다.
            method: METHOD_EVENT_DISPATCH.to_string(),
            params: serde_json::to_value(&params).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// 권한 패턴이 요청 패턴을 "포함"하는지. 매니페스트 검증된 패턴만 받는다.
///
/// - 권한이 `foo.*`: 같은 namespace 안의 모든 정확 키 또는 wildcard 허용
/// - 권한이 정확 키: 같은 정확 키만 허용
fn pattern_covers(allowed: &str, requested: &str) -> bool {
    if allowed == requested {
        return true;
    }
    if let Some(prefix) = allowed.strip_suffix(".*") {
        // 요청도 같은 namespace 하위라면 OK.
        if let Some(req_prefix) = requested.strip_suffix(".*") {
            // foo.* covers foo.* and foo.bar.* (sub-namespace) — 1.0은 한 depth만 고려.
            req_prefix == prefix || req_prefix.starts_with(&format!("{prefix}."))
        } else {
            // 정확 키가 권한 namespace 안에 있는지.
            requested
                .strip_prefix(prefix)
                .map(|rest| rest.starts_with('.') && rest.len() > 1)
                .unwrap_or(false)
        }
    } else {
        false
    }
}

#[derive(Debug)]
pub enum EventBusError {
    SubscribeDenied { plugin_id: String, pattern: String },
    PublishDenied { plugin_id: String, key: String },
    OriginMismatch { plugin_id: String, envelope_origin: EventOrigin },
    HopExceeded { key: String, hop: u8 },
}

impl std::fmt::Display for EventBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubscribeDenied { plugin_id, pattern } => write!(
                f,
                "plugin '{plugin_id}' has no manifest event_subscribe permission for '{pattern}'"
            ),
            Self::PublishDenied { plugin_id, key } => write!(
                f,
                "plugin '{plugin_id}' has no manifest event_publish permission for key '{key}'"
            ),
            Self::OriginMismatch {
                plugin_id,
                envelope_origin,
            } => write!(
                f,
                "plugin '{plugin_id}' publish envelope origin mismatch: {envelope_origin:?}"
            ),
            Self::HopExceeded { key, hop } => {
                write!(f, "event '{key}' hop count {hop} exceeds MAX_HOP")
            }
        }
    }
}

impl std::error::Error for EventBusError {}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_protocol::{EventMeta, EventScope};

    fn env(key: &str, origin: EventOrigin) -> EventEnvelope {
        EventEnvelope {
            key: key.to_string(),
            payload: serde_json::Value::Null,
            meta: EventMeta {
                trace_id: "t".into(),
                hop: 0,
                origin,
                scope: EventScope::System,
            },
        }
    }

    #[test]
    fn exact_pattern_matches_exact_key() {
        assert!(pattern_matches("surface.created", "surface.created"));
        assert!(!pattern_matches("surface.created", "surface.closed"));
    }

    #[test]
    fn wildcard_matches_same_namespace() {
        assert!(pattern_matches("surface.*", "surface.created"));
        assert!(pattern_matches("surface.*", "surface.lifecycle.changed"));
        assert!(!pattern_matches("surface.*", "tab.created"));
        // wildcard는 자기 자신과 같은 namespace 키와는 매칭되지 않음 (.*은 sub-key 의미).
        assert!(!pattern_matches("surface.*", "surface"));
    }

    #[test]
    fn host_publish_fans_out_to_host_subscriber() {
        let bus = EventBus::new();
        let (tx, rx) = mpsc::channel();
        bus.subscribe_host("surface.*", tx);
        let dispatches = bus.publish_from_host(env("surface.created", EventOrigin::Host));
        assert!(dispatches.is_empty()); // plugin 구독자 없음
        let got = rx.try_recv().expect("host subscriber should receive");
        assert_eq!(got.key, "surface.created");
    }

    #[test]
    fn plugin_publish_requires_publish_permission() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec![], vec!["p1.foo.*".into()]);
        let envelope = env(
            "p1.foo.bar",
            EventOrigin::Plugin {
                plugin_id: "p1".into(),
            },
        );
        let res = bus.publish_from_plugin("p1", envelope);
        assert!(res.is_ok());
    }

    #[test]
    fn plugin_publish_rejected_without_permission() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec![], vec![]);
        let envelope = env(
            "p1.foo.bar",
            EventOrigin::Plugin {
                plugin_id: "p1".into(),
            },
        );
        let err = bus.publish_from_plugin("p1", envelope).unwrap_err();
        assert!(matches!(err, EventBusError::PublishDenied { .. }));
    }

    #[test]
    fn plugin_publish_rejected_for_wrong_origin() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec![], vec!["p1.foo.*".into()]);
        let envelope = env(
            "p1.foo.bar",
            EventOrigin::Plugin {
                plugin_id: "p2".into(),
            },
        );
        let err = bus.publish_from_plugin("p1", envelope).unwrap_err();
        assert!(matches!(err, EventBusError::OriginMismatch { .. }));
    }

    #[test]
    fn plugin_publish_rejected_at_hop_overflow() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec![], vec!["p1.foo.*".into()]);
        let mut envelope = env(
            "p1.foo.bar",
            EventOrigin::Plugin {
                plugin_id: "p1".into(),
            },
        );
        envelope.meta.hop = MAX_HOP + 1;
        let err = bus.publish_from_plugin("p1", envelope).unwrap_err();
        assert!(matches!(err, EventBusError::HopExceeded { .. }));
    }

    #[test]
    fn plugin_subscribe_requires_permission() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec!["surface.*".into()], vec![]);
        assert!(bus.subscribe_plugin("p1", 1, "surface.created".into()).is_ok());
        assert!(bus.subscribe_plugin("p1", 2, "tab.created".into()).is_err());
        assert!(bus.subscribe_plugin("p1", 3, "surface.*".into()).is_ok());
    }

    #[test]
    fn fan_out_to_plugin_subscribers_excludes_publisher() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec!["evt.*".into()], vec!["evt.*".into()]);
        bus.set_plugin_permissions("p2", vec!["evt.*".into()], vec![]);
        bus.subscribe_plugin("p1", 1, "evt.*".into()).unwrap();
        bus.subscribe_plugin("p2", 1, "evt.*".into()).unwrap();
        let envelope = env(
            "evt.something",
            EventOrigin::Plugin {
                plugin_id: "p1".into(),
            },
        );
        let dispatches = bus.publish_from_plugin("p1", envelope).unwrap();
        // p1은 자기 이벤트를 다시 받지 않고, p2만 받는다.
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].plugin_id, "p2");
    }

    #[test]
    fn clear_plugin_removes_subs_and_perms() {
        let bus = EventBus::new();
        bus.set_plugin_permissions("p1", vec!["evt.*".into()], vec!["evt.*".into()]);
        bus.subscribe_plugin("p1", 1, "evt.*".into()).unwrap();
        bus.clear_plugin("p1");
        let envelope = env(
            "evt.x",
            EventOrigin::Plugin {
                plugin_id: "p1".into(),
            },
        );
        let res = bus.publish_from_plugin("p1", envelope);
        assert!(matches!(res, Err(EventBusError::PublishDenied { .. })));
    }
}
