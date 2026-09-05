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
#[cfg(debug_assertions)]
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tasty_plugin_protocol::{
    EventDispatchParams, EventEnvelope, EventOrigin, MAX_HOP, METHOD_EVENT_DISPATCH, PluginRequest,
};

/// 패턴 매칭 헬퍼. 검증된 패턴은 정확 key 또는 `<segs>.*` 형태로 정규화돼 있다고 가정.
pub(crate) fn pattern_matches(pattern: &str, key: &str) -> bool {
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

struct PluginSubscription {
    plugin_id: String,
    sub_id: u64,
    pattern: String,
}

/// EventBus 내부 상태. 락 하나로 모든 구독 테이블을 보호.
struct Inner {
    plugin_subs: Vec<PluginSubscription>,
    /// 매니페스트의 `event_subscribe` 패턴 (plugin_id → 패턴 목록).
    plugin_subscribe_perms: HashMap<String, Vec<String>>,
    /// 매니페스트의 `event_publish` 패턴 (plugin_id → 패턴 목록).
    plugin_publish_perms: HashMap<String, Vec<String>>,
    /// debug 빌드 한정 — 최근 발화된 envelope 링버퍼. `debug.event_bus.trace`
    /// CLI가 trace_id로 조회.
    #[cfg(debug_assertions)]
    trace_ring: VecDeque<EventEnvelope>,
}

#[cfg(debug_assertions)]
const TRACE_RING_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<Inner>>,
    /// poison 을 이미 보고했는가. poison 은 sticky 라 fan-out 마다 같은 로그가
    /// 나오는 것을 막는다.
    poison_reported: Arc<AtomicBool>,
}

/// fan-out 결과. 호스트의 plugin 송신 루프가 후처리한다.
/// 락 안에서 `Vec<PluginRequest>`를 만들지 않고 (plugin_id, request_id, EventDispatchParams) 페어만 모아준다.
#[derive(Debug)]
pub struct PluginDispatch {
    pub plugin_id: String,
    pub sub_id: u64,
    pub envelope: EventEnvelope,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Poison 된 버스 상태를 복구한다.
    ///
    /// `Inner` 는 구독 목록과 권한 맵뿐이고 임계구역은 `insert`/`remove`/`retain`/`push`
    /// 밖에 하지 않는다 — 콜백도, 외부 호출도 없다. 그래서 패닉이 나도 맵의 불변식은
    /// 성립하고 데이터는 그대로 쓸 수 있다. 반면 이 버스는 `PluginManager` 가 소유해
    /// **메인 스레드**에서 fan-out 되므로, 여기서 패닉하면 모든 창의 터미널 세션이
    /// 함께 죽는다 — 사망 범위가 비교가 안 된다
    /// ([`error-handling.md`](../../../docs/dev-guide/error-handling.md) "락 poison").
    fn lock_recovering(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| {
            if !self.poison_reported.swap(true, Ordering::Relaxed) {
                tracing::error!(
                    "event bus mutex poisoned — a thread panicked while holding it. Recovering \
                     (subscription and permission maps keep their invariants); later occurrences \
                     are not logged."
                );
            }
            poisoned.into_inner()
        })
    }

    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                plugin_subs: Vec::new(),
                plugin_subscribe_perms: HashMap::new(),
                plugin_publish_perms: HashMap::new(),
                #[cfg(debug_assertions)]
                trace_ring: VecDeque::with_capacity(TRACE_RING_CAPACITY),
            })),
            poison_reported: Arc::new(AtomicBool::new(false)),
        }
    }

    /// plugin이 호스트에 등록될 때 매니페스트의 권한을 적재한다. 비활성화/언인스톨 시 `clear_plugin`으로 정리.
    pub fn set_plugin_permissions(
        &self,
        plugin_id: &str,
        subscribe_patterns: Vec<String>,
        publish_patterns: Vec<String>,
    ) {
        let mut inner = self.lock_recovering();
        inner
            .plugin_subscribe_perms
            .insert(plugin_id.to_string(), subscribe_patterns);
        inner
            .plugin_publish_perms
            .insert(plugin_id.to_string(), publish_patterns);
    }

    /// plugin이 종료되거나 비활성화되면 권한 + 구독 모두 제거.
    pub fn clear_plugin(&self, plugin_id: &str) {
        let mut inner = self.lock_recovering();
        inner.plugin_subscribe_perms.remove(plugin_id);
        inner.plugin_publish_perms.remove(plugin_id);
        inner.plugin_subs.retain(|s| s.plugin_id != plugin_id);
    }

    /// plugin이 `event.subscribe` IPC로 등록한 구독. 매니페스트 권한과 매칭되지 않으면 `Err`.
    /// 같은 `(plugin_id, sub_id)` 페어로 다시 호출되면 마지막 호출이 이긴다.
    pub fn subscribe_plugin(
        &self,
        plugin_id: &str,
        sub_id: u64,
        pattern: String,
    ) -> Result<(), EventBusError> {
        let mut inner = self.lock_recovering();
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
        let mut inner = self.lock_recovering();
        inner
            .plugin_subs
            .retain(|s| !(s.plugin_id == plugin_id && s.sub_id == sub_id));
    }

    /// 호스트 본문이 새 envelope를 발화. 호스트는 모든 namespace에 publish 가능.
    /// 매칭되는 구독자에게 fan-out하고, plugin 측 dispatch 큐를 반환한다.
    pub fn publish_from_host(&self, envelope: EventEnvelope) -> Vec<PluginDispatch> {
        self.fan_out(envelope, None)
    }

    /// owner unicast — envelope를 정확히 한 plugin에만 전달한다. 일반 fan-out과 달리
    /// 호스트/다른 plugin 구독자는 무시. `command.invoked`처럼 의도적으로 owner만 받아야 하는
    /// 이벤트에 사용한다. 구독 권한도 검사하지 않는다 (호스트가 명시적으로 보내는 메시지).
    /// 반환값은 송신용 [`PluginDispatch`] (sub_id=0 sentinel).
    pub fn unicast_to_plugin(&self, plugin_id: &str, envelope: EventEnvelope) -> PluginDispatch {
        PluginDispatch {
            plugin_id: plugin_id.to_string(),
            sub_id: 0,
            envelope,
        }
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
            let inner = self.lock_recovering();
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
        // 이유: 아래 `inner` 를 변형하는 곳이 debug 전용 trace 기록뿐이라 release 에선 `mut` 가 남는다.
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut inner = self.lock_recovering();
        // debug 빌드: trace 링버퍼에 envelope 기록.
        #[cfg(debug_assertions)]
        {
            if inner.trace_ring.len() == TRACE_RING_CAPACITY {
                inner.trace_ring.pop_front();
            }
            inner.trace_ring.push_back(envelope.clone());
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

    /// debug 한정 — 주어진 key에 매칭되는 plugin 구독을 모아 반환.
    /// `(plugin_id, sub_id, 매니페스트의 구독 패턴)`.
    #[cfg(debug_assertions)]
    pub fn debug_list_subscribers(&self, key: &str) -> Vec<(String, u64, String)> {
        let inner = self.lock_recovering();
        inner
            .plugin_subs
            .iter()
            .filter(|s| pattern_matches(&s.pattern, key))
            .map(|s| (s.plugin_id.clone(), s.sub_id, s.pattern.clone()))
            .collect()
    }

    /// debug 한정 — 링버퍼에서 `trace_id`가 일치하는 envelope들을 발화 순서로 반환.
    #[cfg(debug_assertions)]
    pub fn debug_trace(&self, trace_id: &str) -> Vec<EventEnvelope> {
        let inner = self.lock_recovering();
        inner
            .trace_ring
            .iter()
            .filter(|e| e.meta.trace_id == trace_id)
            .cloned()
            .collect()
    }

    /// 테스트 전용 — 락을 든 채 패닉하는 스레드를 띄워 버스를 poison 시킨다.
    ///
    /// 프로덕션 임계구역에는 패닉 지점이 없어(순수 자료구조 조작) 바깥에서 poison 을
    /// 만들 방법이 없다. 그래서 poison 이후에도 버스가 동작하는지 검증하려면 이런
    /// 주입 지점이 필요하다.
    #[cfg(test)]
    pub(crate) fn poison_for_test(&self) {
        let held = Arc::clone(&self.inner);
        let joined = std::thread::spawn(move || {
            let _guard = held.lock().expect("fresh mutex");
            panic!("a thread dies while holding the event bus");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(self.inner.lock().is_err(), "버스가 poison 됐어야 한다");
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
    SubscribeDenied {
        plugin_id: String,
        pattern: String,
    },
    PublishDenied {
        plugin_id: String,
        key: String,
    },
    OriginMismatch {
        plugin_id: String,
        envelope_origin: EventOrigin,
    },
    HopExceeded {
        key: String,
        hop: u8,
    },
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
#[path = "event_bus_tests.rs"]
mod tests;
