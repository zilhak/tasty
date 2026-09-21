//! `EventBus` 재발화 hop 하한 단위 테스트 — plugin 이 적어 보낸 hop 을 믿지 않고,
//! 응답 전인 dispatch 가 있으면 hop 을 올린다(ADR-0406).

use std::collections::VecDeque;

use crate::event_bus::{EventBus, EventBusError, PluginDispatch};
use tasty_plugin_protocol::{EventEnvelope, EventMeta, EventOrigin, EventScope, MAX_HOP};

fn from_plugin(key: &str, plugin_id: &str, hop: u8, trace_id: &str) -> EventEnvelope {
    EventEnvelope {
        key: key.to_string(),
        payload: serde_json::Value::Null,
        meta: EventMeta {
            trace_id: trace_id.into(),
            hop,
            origin: EventOrigin::Plugin {
                plugin_id: plugin_id.into(),
            },
            scope: EventScope::System,
        },
    }
}

fn from_host(key: &str) -> EventEnvelope {
    EventEnvelope {
        key: key.to_string(),
        payload: serde_json::Value::Null,
        meta: EventMeta {
            trace_id: "h1".into(),
            hop: 0,
            origin: EventOrigin::Host,
            scope: EventScope::System,
        },
    }
}

/// 두 plugin 이 서로의 namespace 를 구독하고 자기 namespace 로 발화한다.
fn two_plugins_that_answer_each_other() -> EventBus {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec!["com.b.*".into()], vec!["com.a.*".into()]);
    bus.set_plugin_permissions("com.b", vec!["com.a.*".into()], vec!["com.b.*".into()]);
    bus.subscribe_plugin("com.a", 1, "com.b.*".into()).unwrap();
    bus.subscribe_plugin("com.b", 1, "com.a.*".into()).unwrap();
    bus
}

fn last_hop_in_ring(bus: &EventBus) -> u8 {
    let got = bus.fetch(0, crate::event_bus::EVENT_RING_CAPACITY, None);
    got.events.last().expect("링이 비었다").1.meta.hop
}

/// 서로의 사건에 **hop 0 · 새 trace** 로 반응하는 두 plugin. plugin 이 적은 hop 을
/// 믿으면 이 루프는 영영 안 끝난다 — SDK 의 `publish_fresh` 로 반응하면 바로 이
/// 모양이다. 반응은 dispatch 응답 **전에** 도착한다(SDK 는 `on_event` 를 마친 뒤에
/// 응답한다). 호스트가 하한을 걸면 hop 이 한 번에 하나씩 올라 `MAX_HOP` 을 넘는 순간
/// 끊긴다.
#[test]
fn two_plugins_answering_each_other_with_hop_zero_stop_at_max_hop() {
    let bus = two_plugins_that_answer_each_other();
    let mut queue: VecDeque<PluginDispatch> = bus
        .publish_from_plugin("com.a", from_plugin("com.a.ping", "com.a", 0, "a0"))
        .expect("첫 발화")
        .into();
    let mut next_request_id = 1u64;
    let mut relays = 0usize;
    let stopped_by = loop {
        let d = queue
            .pop_front()
            .expect("루프가 끊기기 전에 dispatch 가 말랐다");
        let request_id = next_request_id;
        next_request_id += 1;
        bus.note_dispatch_sent(&d.plugin_id, request_id, d.envelope.meta.hop);
        let (key, trace) = if d.plugin_id == "com.a" {
            ("com.a.ping", format!("a{relays}"))
        } else {
            ("com.b.pong", format!("b{relays}"))
        };
        let reaction =
            bus.publish_from_plugin(&d.plugin_id, from_plugin(key, &d.plugin_id, 0, &trace));
        bus.note_dispatch_answered(&d.plugin_id, request_id);
        match reaction {
            Ok(more) => {
                relays += 1;
                assert!(relays <= 1000, "하한이 없으면 여기서 끝나지 않는다");
                queue.extend(more);
            }
            Err(e) => break e,
        }
    };
    assert!(
        matches!(stopped_by, EventBusError::HopExceeded { hop, .. } if hop == MAX_HOP + 1),
        "hop 이 MAX_HOP 을 넘어 끊겨야 한다: {stopped_by}"
    );
    assert_eq!(
        relays, MAX_HOP as usize,
        "첫 발화가 hop 0, 반응 하나마다 +1 — hop 1..=MAX_HOP 까지 {} 번 통과한다",
        MAX_HOP
    );
}

/// 호스트 사건(hop 0)을 받고 응답하기 전에 publish 하면 hop 1 이다 — 사건 카탈로그의
/// "plugin 재발화 시 +1" 이 plugin 의 관례가 아니라 호스트가 정하는 값이 된다.
#[test]
fn a_publish_before_answering_a_host_event_gets_hop_one() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec!["agent.*".into()], vec!["com.a.*".into()]);
    bus.subscribe_plugin("com.a", 1, "agent.*".into()).unwrap();
    let ds = bus.publish_from_host(from_host("agent.task_finished"));
    assert_eq!(ds.len(), 1);
    bus.note_dispatch_sent("com.a", 7, ds[0].envelope.meta.hop);
    bus.publish_from_plugin(
        "com.a",
        from_plugin("com.a.relay", "com.a", 0, "forged-fresh"),
    )
    .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 1);
}

/// 응답이 온 뒤의 publish 는 반응이 아니라 새 발화다 — 하한이 없다. 주기적으로
/// 발화하는 plugin 이 사건을 받았다는 이유만으로 hop 이 쌓이지 않는다.
#[test]
fn a_publish_after_the_answer_is_a_fresh_publish() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 7, 5);
    assert!(bus.note_dispatch_answered("com.a", 7));
    bus.publish_from_plugin("com.a", from_plugin("com.a.tick", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 0);
}

/// 하한은 올리기만 한다 — plugin 이 더 큰 hop 을 적었으면 그대로 둔다.
#[test]
fn a_hop_above_the_floor_is_kept() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 1, 2);
    bus.publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 9, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 9);
}

/// 응답을 기다리는 dispatch 가 여럿이면 가장 큰 hop 이 하한을 정한다.
#[test]
fn the_floor_follows_the_highest_hop_still_waiting_for_an_answer() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 1, 3);
    bus.note_dispatch_sent("com.a", 2, 11);
    bus.note_dispatch_sent("com.a", 3, 0);
    bus.publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 12);
    assert!(bus.note_dispatch_answered("com.a", 2));
    bus.publish_from_plugin("com.a", from_plugin("com.a.y", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 4);
}

/// 하한으로 올린 hop 이 `MAX_HOP` 을 넘으면 거절된다 — 검사는 올린 뒤의 값으로 한다.
#[test]
fn a_raised_hop_past_max_hop_is_rejected() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 1, MAX_HOP);
    let err = bus
        .publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 0, "t"))
        .unwrap_err();
    assert!(matches!(err, EventBusError::HopExceeded { hop, .. } if hop == MAX_HOP + 1));
}

/// 다른 plugin 이 받은 dispatch 는 이 plugin 의 하한과 무관하다.
#[test]
fn the_floor_is_per_plugin() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.b", 1, 8);
    bus.publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 0);
}

/// 모르는 응답 id 는 버스의 것이 아니다 — 호출자가 다른 pending 과 견주게 둔다.
#[test]
fn an_answer_the_bus_did_not_record_is_not_claimed() {
    let bus = EventBus::new();
    assert!(!bus.note_dispatch_answered("com.a", 99));
    bus.note_dispatch_sent("com.a", 1, 0);
    assert!(!bus.note_dispatch_answered("com.a", 99));
    assert!(bus.note_dispatch_answered("com.a", 1));
    assert!(
        !bus.note_dispatch_answered("com.a", 1),
        "두 번째 응답은 이미 정리됐다"
    );
}

/// 재시작한 plugin 은 옛 프로세스가 받던 dispatch 에 응답하지 않는다 — 정리하지
/// 않으면 새 프로세스의 publish 가 영영 하한에 걸린다.
#[test]
fn clearing_a_plugin_drops_what_it_was_answering() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 1, 6);
    bus.clear_plugin("com.a");
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(last_hop_in_ring(&bus), 0);
}

/// 응답하지 않는 plugin 에 기록이 끝없이 쌓이지 않는다 — 상한을 넘으면 가장
/// 오래된 것부터 버린다.
#[test]
fn records_for_a_plugin_that_never_answers_are_bounded() {
    let bus = EventBus::new();
    bus.set_plugin_permissions("com.a", vec![], vec!["com.a.*".into()]);
    bus.note_dispatch_sent("com.a", 0, 10);
    for id in 1..=crate::event_bus::EVENT_RING_CAPACITY as u64 {
        bus.note_dispatch_sent("com.a", id, 0);
    }
    assert!(
        !bus.note_dispatch_answered("com.a", 0),
        "가장 오래된 기록은 밀려났어야 한다"
    );
    bus.publish_from_plugin("com.a", from_plugin("com.a.x", "com.a", 0, "t"))
        .unwrap();
    assert_eq!(
        last_hop_in_ring(&bus),
        1,
        "밀려난 hop 10 은 하한에서 빠진다"
    );
}
