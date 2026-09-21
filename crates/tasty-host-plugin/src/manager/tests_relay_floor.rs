//! 재발화 hop 하한의 **manager 배선** — `event.dispatch` 를 보낸 순간 기록하고, 그 응답이
//! 오면 지우고, plugin publish 가 도착한 순간 하한을 건다(ADR-0406). 규칙 자체는
//! `event_bus_relay_tests.rs` 가 고정하고, 여기는 세 자리가 실제로 이어졌는지만 본다.

use std::sync::Arc;

use tasty_plugin_protocol::{EventEnvelope, EventMeta, EventOrigin, EventScope};
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::PluginManager;
use crate::process::PluginProcess;
use crate::protocol::PluginResponse;

const PLUGIN: &str = "com.example.relay";

fn relay(hop: u8) -> EventEnvelope {
    EventEnvelope {
        key: format!("{PLUGIN}.relay"),
        payload: serde_json::Value::Null,
        meta: EventMeta {
            trace_id: "forged-fresh".into(),
            hop,
            origin: EventOrigin::Plugin {
                plugin_id: PLUGIN.into(),
            },
            scope: EventScope::System,
        },
    }
}

fn last_hop(mgr: &PluginManager) -> u8 {
    let got = mgr
        .event_bus
        .fetch(0, crate::event_bus::EVENT_RING_CAPACITY, None);
    got.events.last().expect("링이 비었다").1.meta.hop
}

#[test]
fn a_publish_between_a_dispatch_and_its_answer_is_raised_and_one_after_is_not() {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let (proc, rx) = PluginProcess::stub_with_request_rx(PLUGIN);
    mgr.processes.insert(PLUGIN.into(), proc);
    mgr.event_bus.set_plugin_permissions(
        PLUGIN,
        vec!["agent.*".into()],
        vec![format!("{PLUGIN}.*")],
    );
    mgr.event_bus
        .subscribe_plugin(PLUGIN, 1, "agent.*".into())
        .unwrap();

    mgr.emit_host_event(
        "agent.task_finished",
        &serde_json::json!({ "state": "succeeded" }),
        EventScope::System,
    );
    let sent = rx
        .try_recv()
        .expect("구독한 plugin 에 event.dispatch 가 나가야 한다");
    assert_eq!(sent.method, tasty_plugin_protocol::METHOD_EVENT_DISPATCH);

    // 응답 전 — plugin 이 hop 0 으로 적어 보내도 host 가 1 로 올린다.
    mgr.route_plugin_event_publish(PLUGIN, relay(0));
    assert_eq!(last_hop(&mgr), 1);

    // 그 dispatch 의 응답은 버스가 가져간다 — 그 뒤의 publish 는 새 발화다.
    mgr.handle_plugin_response(
        PLUGIN,
        PluginResponse {
            id: sent.id,
            result: Some(serde_json::Value::Null),
            error: None,
            error_code: None,
        },
    );
    mgr.route_plugin_event_publish(PLUGIN, relay(0));
    assert_eq!(last_hop(&mgr), 0);
}
