use super::*;
use crate::stream::StreamTag;
const FULL: usize = MAX_FRAME_LEN as usize + FRAME_HEADER_LEN;
fn frame(n: usize) -> StreamFrame {
    StreamFrame::new(StreamTag::Control, vec![b'x'; n])
}
fn setup() -> (StreamHub, SinkReceiver, Weak<()>) {
    let hub = StreamHub::new();
    let rx = hub.register(5);
    let binding = hub.client_binding(5).unwrap();
    (hub, rx, binding)
}
fn used(hub: &StreamHub) -> usize {
    hub.sinks
        .lock()
        .unwrap()
        .get(&5)
        .unwrap()
        .bytes
        .load(Ordering::Acquire)
}
#[test]
fn reserve_before_poll_reserves_full_frame_and_unused_drop_releases_it() {
    let (hub, _rx, binding) = setup();
    assert!(hub.reserve_frame(5, &binding, FULL - 1).is_none());
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    assert_eq!(a.capacity(), FULL);
    assert_eq!(used(&hub), FULL);
    assert!(hub.reserve_frame(5, &binding, FULL).is_none());
    drop(a);
    assert_eq!(used(&hub), 0);
    assert!(hub.reserve_frame(5, &binding, FULL).is_some());
}
#[test]
fn reconcile_short_frame_and_keep_charge_until_writer_releases_live_frame() {
    let (hub, rx, binding) = setup();
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    assert_eq!(hub.push_reserved(a, frame(17)), PushResult::Sent);
    assert_eq!(used(&hub), 17 + FRAME_HEADER_LEN);
    let held = rx
        .recv_held_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(hub.loss().backlog, 0);
    assert_eq!(held.payload.len(), 17);
    assert_eq!(used(&hub), 17 + FRAME_HEADER_LEN);
    assert!(hub.reserve_frame(5, &binding, FULL).is_none());
    drop(held);
    assert_eq!(used(&hub), 0);
}
#[test]
fn ordinary_queued_and_live_traffic_counts_against_http_reservation() {
    let (hub, rx, binding) = setup();
    assert_eq!(hub.push(5, frame(3)), PushResult::Sent);
    assert!(hub.reserve_frame(5, &binding, FULL).is_none());
    let held = rx
        .recv_held_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(hub.reserve_frame(5, &binding, FULL).is_none());
    drop(held);
    assert!(hub.reserve_frame(5, &binding, FULL).is_some());
}
#[test]
fn replaced_registration_rejects_old_reservation_and_does_not_charge_new_one() {
    let (hub, old_rx, binding) = setup();
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    let old_counter = a.lease.counter.clone();
    let new_rx = hub.register(5);
    assert_eq!(hub.push_reserved(a, frame(3)), PushResult::Unknown);
    assert_eq!(old_counter.load(Ordering::Acquire), 0);
    assert_eq!(used(&hub), 0);
    assert!(old_rx.try_recv().is_err());
    assert!(new_rx.try_recv().is_err());
    assert!(hub.reserve_frame(5, &binding, FULL).is_none());
}
#[test]
fn oversize_frame_releases_reservation_without_enqueue_or_limit_change() {
    let (hub, rx, binding) = setup();
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    assert_eq!(
        hub.push_reserved(a, frame(MAX_FRAME_LEN as usize + 1)),
        PushResult::Unknown
    );
    assert_eq!(used(&hub), 0);
    assert!(rx.try_recv().is_err());
    assert_eq!(MAX_FRAME_LEN, 1 << 20);
}
#[test]
fn full_or_disconnected_queue_releases_reconciled_reservation() {
    let (hub, rx, binding) = setup();
    for _ in 0..SINK_CAP {
        assert_eq!(hub.push(5, frame(0)), PushResult::Sent);
    }
    let queued = SINK_CAP * FRAME_HEADER_LEN;
    let a = hub.reserve_frame(5, &binding, FULL + queued).unwrap();
    assert_eq!(hub.push_reserved(a, frame(17)), PushResult::Dropped);
    assert_eq!(used(&hub), queued);
    drop(rx);
    assert_eq!(used(&hub), 0);
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    let counter = a.lease.counter.clone();
    assert_eq!(hub.push_reserved(a, frame(17)), PushResult::Unknown);
    assert_eq!(counter.load(Ordering::Acquire), 0);
}
#[test]
fn borrowed_production_frame_keeps_charge_through_writer_error() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (hub, rx, binding) = setup();
    let a = hub.reserve_frame(5, &binding, FULL).unwrap();
    assert_eq!(hub.push_reserved(a, frame(17)), PushResult::Sent);
    let held = rx
        .recv_held_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        crate::stream::write_frame(&mut Broken, held.tag, &held.payload)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert_eq!(used(&hub), 17 + FRAME_HEADER_LEN);
    drop(held);
    assert_eq!(used(&hub), 0);
}

#[test]
fn queued_owner_retirement_and_registration_replacement_are_checked_at_write() {
    let (hub, rx, binding) = setup();
    let owner = PublicationFence::default();
    let a = hub
        .reserve_frame(5, &binding, FULL)
        .unwrap()
        .for_owner(&owner);
    assert_eq!(hub.push_reserved(a, frame(17)), PushResult::Sent);
    let held = rx
        .recv_held_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(held.is_current());
    drop(owner);
    assert!(!held.is_current());
    drop(held);
    assert_eq!(used(&hub), 0);
    assert_eq!(hub.push(5, frame(3)), PushResult::Sent);
    let held = rx
        .recv_held_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(held.is_current());
    let _replacement = hub.register(5);
    assert!(!held.is_current());
}
#[test]
fn retired_owner_before_enqueue_cannot_publish_and_releases_full_reservation() {
    let (hub, rx, binding) = setup();
    let owner = PublicationFence::default();
    let a = hub
        .reserve_frame(5, &binding, FULL)
        .unwrap()
        .for_owner(&owner);
    drop(owner);
    assert_eq!(hub.push_reserved(a, frame(3)), PushResult::Unknown);
    assert_eq!(used(&hub), 0);
    assert!(rx.try_recv().is_err());
}
