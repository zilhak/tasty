//! `EventBus` 단위 테스트.

#![cfg(test)]

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

#[cfg(debug_assertions)]
#[test]
fn debug_list_subscribers_matches_subscribed_plugins() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("p1", vec!["surface.*".into()], vec![]);
    bus.set_plugin_permissions("p2", vec!["surface.closed".into()], vec![]);
    bus.subscribe_plugin("p1", 1, "surface.*".into()).unwrap();
    bus.subscribe_plugin("p2", 7, "surface.closed".into()).unwrap();
    let subs = bus.debug_list_subscribers("surface.closed");
    assert_eq!(subs.len(), 2);
    assert!(subs.iter().any(|(p, _, _)| p == "p1"));
    assert!(subs.iter().any(|(p, sub, _)| p == "p2" && *sub == 7));
    let none = bus.debug_list_subscribers("tab.created");
    assert!(none.is_empty());
}

#[cfg(debug_assertions)]
#[test]
fn debug_trace_returns_recent_envelopes_by_id() {
    let bus = EventBus::new();
    // 3건 발화 — 같은 trace_id 2건 + 다른 1건.
    let mut e1 = env("surface.created", EventOrigin::Host);
    e1.meta.trace_id = "h1".into();
    let mut e2 = env("surface.closed", EventOrigin::Host);
    e2.meta.trace_id = "h1".into();
    let mut e3 = env("tab.created", EventOrigin::Host);
    e3.meta.trace_id = "h2".into();
    bus.publish_from_host(e1);
    bus.publish_from_host(e2);
    bus.publish_from_host(e3);
    let chain = bus.debug_trace("h1");
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].key, "surface.created");
    assert_eq!(chain[1].key, "surface.closed");
    let other = bus.debug_trace("h99");
    assert!(other.is_empty());
}

#[test]
fn unicast_to_plugin_bypasses_subscribers_and_uses_zero_sub_id() {
    // unicast는 fan-out과 별개 경로. 호스트/다른 plugin이 구독해도 envelope를 받지 않는다.
    let bus = EventBus::new();
    let (tx, rx) = mpsc::channel();
    bus.subscribe_host("command.*", tx);
    bus.set_plugin_permissions("p2", vec!["command.*".into()], vec![]);
    bus.subscribe_plugin("p2", 1, "command.*".into()).unwrap();
    let envelope = env("command.invoked", EventOrigin::Host);
    let dispatch = bus.unicast_to_plugin("p1", envelope);
    assert_eq!(dispatch.plugin_id, "p1");
    assert_eq!(dispatch.sub_id, 0);
    // 호스트/p2 구독자는 아무것도 받지 않는다.
    assert!(rx.try_recv().is_err());
}
