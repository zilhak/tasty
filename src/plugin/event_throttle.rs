//! 고빈도 이벤트(`surface.resized`, `split.ratio_changed`)용 호스트 측 throttle.
//!
//! 정책: leading + trailing, 150ms 윈도우.
//! - leading: 키-스코프 페어로 마지막 발화 후 150ms가 지났으면 즉시 발화.
//! - 윈도우가 살아있는 동안의 호출은 마지막 payload만 보관 (`pending`).
//! - trailing: 윈도우 만료 후 첫 tick에서 보관해 둔 `pending`을 1회 발화.
//!
//! `pump_trailing`은 호스트 main loop이 매 tick 호출해 만료된 pending을 비운다.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tasty_plugin_protocol::EventEnvelope;

/// throttle 윈도우 길이.
pub const THROTTLE_WINDOW: Duration = Duration::from_millis(150);

/// `(이벤트 키, 스코프 키)` — 같은 키여도 surface_id가 다르면 별개로 throttle한다.
/// `surface.resized`의 스코프 키는 surface_id, `split.ratio_changed`는 group_id 등.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ThrottleKey {
    pub event_key: String,
    pub scope_id: u64,
}

#[derive(Debug, Default)]
pub struct EventThrottler {
    last_fired: HashMap<ThrottleKey, Instant>,
    pending: HashMap<ThrottleKey, EventEnvelope>,
}

/// `attempt`의 결과 — 호출자에게 즉시 발화할 envelope을 돌려준다.
/// trailing은 [`EventThrottler::drain_due`]가 별도 tick에서 모아 반환.
#[derive(Debug)]
pub enum ThrottleDecision {
    /// 윈도우가 비어 있어서 즉시 발화 가능. 호출자는 envelope를 그대로 publish.
    EmitNow(EventEnvelope),
    /// 윈도우 진행 중. envelope은 trailing 발화용으로 보관됨.
    Deferred,
}

impl EventThrottler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attempt(&mut self, key: ThrottleKey, envelope: EventEnvelope) -> ThrottleDecision {
        self.attempt_at(key, envelope, Instant::now())
    }

    fn attempt_at(
        &mut self,
        key: ThrottleKey,
        envelope: EventEnvelope,
        now: Instant,
    ) -> ThrottleDecision {
        let can_emit_now = match self.last_fired.get(&key) {
            Some(t) => now.duration_since(*t) >= THROTTLE_WINDOW,
            None => true,
        };
        if can_emit_now {
            self.last_fired.insert(key.clone(), now);
            self.pending.remove(&key);
            ThrottleDecision::EmitNow(envelope)
        } else {
            self.pending.insert(key, envelope);
            ThrottleDecision::Deferred
        }
    }

    /// 윈도우 만료된 pending들을 모아 반환. 호스트 main tick에서 1회 호출.
    pub fn drain_due(&mut self) -> Vec<EventEnvelope> {
        self.drain_due_at(Instant::now())
    }

    fn drain_due_at(&mut self, now: Instant) -> Vec<EventEnvelope> {
        let mut out: Vec<EventEnvelope> = Vec::new();
        let due_keys: Vec<ThrottleKey> = self
            .pending
            .keys()
            .filter(|k| {
                self.last_fired
                    .get(k)
                    .map(|t| now.duration_since(*t) >= THROTTLE_WINDOW)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        for k in due_keys {
            if let Some(env) = self.pending.remove(&k) {
                self.last_fired.insert(k, now);
                out.push(env);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_protocol::{EventMeta, EventOrigin, EventScope};

    fn env(key: &str, n: u64) -> EventEnvelope {
        EventEnvelope {
            key: key.to_string(),
            payload: serde_json::json!({ "n": n }),
            meta: EventMeta {
                trace_id: format!("t{n}"),
                hop: 0,
                origin: EventOrigin::Host,
                scope: EventScope::Surface,
            },
        }
    }

    fn k(key: &str, id: u64) -> ThrottleKey {
        ThrottleKey {
            event_key: key.into(),
            scope_id: id,
        }
    }

    #[test]
    fn first_attempt_emits_now() {
        let mut t = EventThrottler::new();
        let now = Instant::now();
        match t.attempt_at(k("surface.resized", 1), env("surface.resized", 1), now) {
            ThrottleDecision::EmitNow(e) => assert_eq!(e.payload["n"], 1),
            _ => panic!("first should emit"),
        }
    }

    #[test]
    fn within_window_defers_and_overwrites_pending() {
        let mut t = EventThrottler::new();
        let t0 = Instant::now();
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 1), t0);
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 2), t0);
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 3), t0);
        // 윈도우 안 → 마지막 envelope이 pending에 남아 있어야 함.
        let due = t.drain_due_at(t0 + Duration::from_millis(50));
        assert!(due.is_empty()); // 만료 전 — 비어야 함
        let due = t.drain_due_at(t0 + THROTTLE_WINDOW);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].payload["n"], 3);
    }

    #[test]
    fn separate_scope_ids_dont_block_each_other() {
        let mut t = EventThrottler::new();
        let t0 = Instant::now();
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 1), t0);
        match t.attempt_at(k("surface.resized", 2), env("surface.resized", 2), t0) {
            ThrottleDecision::EmitNow(_) => {}
            _ => panic!("different scope should emit immediately"),
        }
    }

    #[test]
    fn after_window_passes_attempt_emits_again() {
        let mut t = EventThrottler::new();
        let t0 = Instant::now();
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 1), t0);
        match t.attempt_at(
            k("surface.resized", 1),
            env("surface.resized", 9),
            t0 + THROTTLE_WINDOW,
        ) {
            ThrottleDecision::EmitNow(e) => assert_eq!(e.payload["n"], 9),
            _ => panic!("after window should emit"),
        }
    }

    #[test]
    fn drain_due_clears_pending() {
        let mut t = EventThrottler::new();
        let t0 = Instant::now();
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 1), t0);
        t.attempt_at(k("surface.resized", 1), env("surface.resized", 2), t0);
        let due = t.drain_due_at(t0 + THROTTLE_WINDOW);
        assert_eq!(due.len(), 1);
        // 다음 drain은 비어 있어야 함 (재발화는 새 attempt 필요).
        let due = t.drain_due_at(t0 + THROTTLE_WINDOW + Duration::from_millis(100));
        assert!(due.is_empty());
    }
}
