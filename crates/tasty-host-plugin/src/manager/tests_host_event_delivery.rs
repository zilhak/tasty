//! 호스트가 plugin 하나에 보낸 이벤트가 송신 큐에 들어갔는지 돌려받는지 확인한다.
//! 큐 포화는 용량 하나짜리 stub 큐에 첫 이벤트를 남겨 흉내 낸다.

use std::sync::Arc;

use tasty_plugin_protocol::EventScope;
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{HostEventDelivery, PluginManager};
use crate::process::PluginProcess;

const ID: &str = "com.example.owner";

fn send(m: &mut PluginManager, plugin_id: &str, n: u64) -> HostEventDelivery {
    m.try_emit_host_event_to_plugin(
        plugin_id,
        "example.event",
        &serde_json::json!({ "n": n }),
        EventScope::System,
    )
}

/// Given 한 이벤트만 받는 plugin 송신 큐, When 이벤트를 둘 보내면, Then 첫째는 큐에 들어가고 둘째는 거절된다.
/// 큐를 비우면 같은 이벤트가 다시 들어간다.
#[test]
fn an_event_refused_by_a_full_queue_is_reported_and_fits_once_the_queue_drains() {
    let mut m = PluginManager::new(Arc::new(NoopWakerFactory));
    let (process, tap) = PluginProcess::stub_with_request_rx_capacity(ID, 1);
    m.processes.insert(ID.into(), process);
    assert_eq!(send(&mut m, ID, 1), HostEventDelivery::Queued);
    assert_eq!(send(&mut m, ID, 2), HostEventDelivery::Refused);
    let first = tap.try_recv().expect("the first event is in the queue");
    assert_eq!(first.params["envelope"]["payload"]["n"], 1);
    assert_eq!(send(&mut m, ID, 2), HostEventDelivery::Queued);
}

/// Given 실행 중이 아닌 plugin, When 이벤트를 보내면, Then 다시 보내도 받을 곳이 없다고 알린다.
#[test]
fn an_event_for_a_plugin_that_is_not_running_is_unreachable() {
    let mut m = PluginManager::new(Arc::new(NoopWakerFactory));
    assert_eq!(send(&mut m, ID, 1), HostEventDelivery::Unreachable);
}
