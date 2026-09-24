use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;

fn small(queue: usize, total: usize) -> Arc<ChannelLedger> {
    ChannelLedger::new(ChannelLimits {
        queue_bytes: queue,
        total_bytes: total,
    })
}

/// 빈 큐는 큰 메시지 하나도 받고, 그 이후부터 누적 상한을 적용한다.
#[test]
fn a_queue_refuses_past_its_bytes_but_an_empty_queue_takes_one() {
    let ledger = small(100, 10_000);
    let (tx, rx) = metered_channel::<u8>(16, ledger.open_queue("p", Direction::Request));

    assert_eq!(
        tx.try_send(1, 500, Admission::Data),
        Ok(()),
        "빈 큐는 상한보다 큰 한 건을 받는다"
    );
    assert_eq!(
        tx.try_send(2, 1, Admission::Data),
        Err(TrySendRefusal::Bytes(Refusal::Queue)),
        "상한을 넘은 큐에 더 들어갔다"
    );

    assert_eq!(rx.try_recv(), Ok(1));
    assert_eq!(tx.try_send(3, 60, Admission::Data), Ok(()));
    assert_eq!(
        tx.try_send(4, 40, Admission::Data),
        Ok(()),
        "상한과 같으면 들어간다"
    );
    assert_eq!(
        tx.try_send(5, 1, Admission::Data),
        Err(TrySendRefusal::Bytes(Refusal::Queue))
    );
    assert_eq!(ledger.snapshot().refused_over_queue, 2);
}

/// 합계는 큐를 가로지른다 — 두 plugin 이 각자 큐 상한 안이어도 합이 넘으면 거절한다.
#[test]
fn the_total_is_shared_across_queues() {
    let ledger = small(1_000, 150);
    let (a, _ra) = metered_channel::<u8>(16, ledger.open_queue("a", Direction::Request));
    let (b, _rb) = metered_channel::<u8>(16, ledger.open_queue("b", Direction::Request));

    a.try_send(1, 100, Admission::Data).unwrap();
    b.try_send(1, 40, Admission::Data).unwrap();
    a.try_send(2, 10, Admission::Data).unwrap();
    assert_eq!(
        b.try_send(2, 1, Admission::Data),
        Err(TrySendRefusal::Bytes(Refusal::Total)),
        "합계가 상한인데 또 들어갔다"
    );
    let snap = ledger.snapshot();
    assert_eq!(snap.total_bytes, 150);
    assert_eq!(snap.refused_over_total, 1);
}

/// 수신단이 사라지면 남은 바이트도 전체 합계에서 빼야 한다.
#[test]
fn dropping_the_receiver_returns_its_bytes_to_the_total() {
    let ledger = small(1_000, 1_000);
    let (tx, rx) = metered_channel::<u8>(16, ledger.open_queue("gone", Direction::Event));
    tx.try_send(1, 300, Admission::Data).unwrap();
    tx.try_send(2, 200, Admission::Data).unwrap();
    assert_eq!(ledger.snapshot().total_bytes, 500);

    drop(rx);
    let snap = ledger.snapshot();
    assert_eq!(snap.total_bytes, 0, "닫힌 큐의 몫이 합계에 남았다");
    assert!(snap.queues.is_empty(), "닫힌 큐가 장부에 남았다");
    assert_eq!(snap.peak_total_bytes, 500, "최댓값은 닫혀도 남는다");
    assert_eq!(
        tx.try_send(3, 1, Admission::Data),
        Err(TrySendRefusal::Disconnected),
        "닫힌 큐가 받았다"
    );
}

/// 닫은 뒤 늦게 오는 release 는 합계를 두 번 빼지 않는다 — 닫을 때 이미 통째로 뺐다.
#[test]
fn a_release_after_close_does_not_subtract_twice() {
    let ledger = small(1_000, 1_000);
    let meter = ledger.open_queue("x", Direction::Request);
    let (other_tx, _other_rx) =
        metered_channel::<u8>(16, ledger.open_queue("y", Direction::Request));
    meter.try_reserve(100).unwrap();
    other_tx.try_send(1, 70, Admission::Data).unwrap();
    meter.close();
    meter.release(100);
    assert_eq!(
        ledger.snapshot().total_bytes,
        70,
        "다른 큐의 바이트까지 차감됐다"
    );
}

/// 상한 때문에 기다리기 전에 호스트를 깨우고, 여유가 생기면 전송해야 한다.
#[test]
fn the_waiting_direction_stands_until_room_is_made() {
    let ledger = small(100, 10_000);
    let (tx, rx) = metered_channel::<u32>(16, ledger.open_queue("slow", Direction::Response));
    tx.send_waiting(1, 100, || {}).unwrap();

    let woke = Arc::new(AtomicUsize::new(0));
    let woke_in = woke.clone();
    let writer = std::thread::spawn(move || {
        tx.send_waiting(2, 50, || {
            woke_in.fetch_add(1, Ordering::Relaxed);
        })
    });

    // 잠깐 기다린 뒤에도 두 번째 항목이 아직 전송되지 않았는지 확인한다.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        ledger.snapshot().queues[0].queued_messages,
        1,
        "상한을 넘겨 들어갔다"
    );

    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(1));
    assert_eq!(writer.join().unwrap(), Ok(()));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(2));
    assert!(
        woke.load(Ordering::Relaxed) >= 1,
        "대기 전에 pump를 깨우지 않았다"
    );
    let snap = ledger.snapshot();
    assert_eq!(snap.waits, 1);
    assert_eq!(snap.total_bytes, 0);
}

/// 기다리는 중에 받는 쪽이 사라지면 영영 자지 않고 끝난다.
#[test]
fn a_waiter_is_released_when_the_receiver_goes_away() {
    let ledger = small(100, 10_000);
    let (tx, rx) = metered_channel::<u32>(16, ledger.open_queue("dying", Direction::Event));
    tx.send_waiting(1, 100, || {}).unwrap();
    let writer = std::thread::spawn(move || tx.send_waiting(2, 50, || {}));
    std::thread::sleep(Duration::from_millis(50));
    drop(rx);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // 의도적 무시: timeout으로 시험이 실패해 수신단이 사라졌으면 더 회신할 필요가 없다.
        let _ = done_tx.send(writer.join().unwrap());
    });
    assert_eq!(
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("받는 쪽이 사라졌는데 기다리던 쪽이 안 돌아왔다"),
        Err(Refusal::Closed)
    );
}

/// 기본 전체 상한은 큐별 상한보다 크고, 번들 플러그인의 큐별 상한 합보다 작다.
#[test]
fn the_two_default_limits_measure_different_things() {
    const {
        assert!(TOTAL_BYTES_LIMIT > QUEUE_BYTES_LIMIT);
        assert!(TOTAL_BYTES_LIMIT < QUEUE_BYTES_LIMIT * 3 * 9);
    };
    assert_eq!(ChannelLimits::default().queue_bytes, QUEUE_BYTES_LIMIT);
    assert_eq!(ChannelLimits::default().total_bytes, TOTAL_BYTES_LIMIT);
}
