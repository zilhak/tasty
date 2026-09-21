use std::time::Duration;

use super::*;

const SLOW: Duration = Duration::from_millis(150);
const FAST: Duration = Duration::from_millis(1);

fn host(seq: u64, queue_wait: Duration, host: Duration) -> HostLeg<'static> {
    HostLeg {
        request_seq: seq,
        method: "workspace.list",
        caller: CallerKind::Local,
        queue_wait,
        host,
    }
}

fn hop(req_id: u64, wait: Duration, outcome: HopOutcome) -> PluginHop {
    PluginHop {
        plugin_id: "com.example.owner".into(),
        host_request_id: req_id,
        wait_us: as_micros(wait),
        outcome,
    }
}

fn seqs(log: &SlowRequestLog) -> Vec<u64> {
    log.snapshot().rows.iter().map(|r| r.request_seq).collect()
}

/// 문턱 아래 요청은 링에 안 든다 — 정상 요청이 원인 요청을 밀어내지 않게 하는 것이 링의
/// 전제다. 큐 대기와 호스트 처리의 **합**으로 판정한다: 어느 한쪽만 느려도 든다.
#[test]
fn only_requests_at_or_above_the_threshold_are_kept() {
    let log = SlowRequestLog::default();
    log.finish_host(host(1, FAST, FAST));
    log.finish_host(host(2, SLOW, FAST));
    log.finish_host(host(3, FAST, SLOW));
    let just_under = SLOW_REQUEST_THRESHOLD - Duration::from_micros(1);
    log.finish_host(host(4, just_under, Duration::ZERO));
    log.finish_host(host(5, SLOW_REQUEST_THRESHOLD, Duration::ZERO));
    assert_eq!(seqs(&log), [2, 3, 5]);
    let row = &log.snapshot().rows[0];
    let h = row.host.as_ref().expect("host part");
    assert_eq!(h.method, "workspace.list");
    assert_eq!(h.queue_wait_us, 150_000);
    assert!(row.plugin_hops.is_empty());
}

/// 넘치면 가장 먼저 든 줄이 밀려나고, 밀려난 수는 `admitted` 와 줄 수의 차로 남는다.
#[test]
fn a_full_ring_pushes_out_its_oldest_row() {
    let log = SlowRequestLog::default();
    let n = SLOW_REQUEST_CAPACITY as u64 + 3;
    for seq in 1..=n {
        log.finish_host(host(seq, SLOW, FAST));
    }
    let snap = log.snapshot();
    assert_eq!(snap.rows.len(), SLOW_REQUEST_CAPACITY);
    assert_eq!(snap.admitted, n);
    assert_eq!(snap.rows.first().map(|r| r.request_seq), Some(4));
    assert_eq!(snap.rows.last().map(|r| r.request_seq), Some(n));
}

/// plugin 으로 넘긴 요청은 호스트 몫이 빨라도 plugin 대기가 길면 **한 줄로** 든다 — 호스트
/// 몫과 hop 이 같은 요청 번호로 이어진다.
#[test]
fn a_forward_joins_its_host_part_and_its_slow_hop_in_one_row() {
    let log = SlowRequestLog::default();
    log.note_forwarded(7);
    log.finish_host(host(7, FAST, FAST));
    assert!(seqs(&log).is_empty(), "호스트 몫만으로는 아직 안 느리다");
    log.finish_plugin_hop(7, hop(41, SLOW, HopOutcome::Expired), true);
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].request_seq, 7);
    assert!(rows[0].host.is_some(), "호스트 몫이 이어지지 않았다");
    assert_eq!(rows[0].plugin_hops, [hop(41, SLOW, HopOutcome::Expired)]);
}

/// 빠르게 끝난 forward 는 열린 자리까지 치워진다 — 아무 데도 안 남는다.
#[test]
fn a_fast_forward_leaves_no_row_and_no_open_entry() {
    let log = SlowRequestLog::default();
    log.note_forwarded(8);
    log.finish_host(host(8, FAST, FAST));
    log.finish_plugin_hop(8, hop(42, FAST, HopOutcome::Ok), true);
    assert!(seqs(&log).is_empty());
    assert!(
        log.lock().open.is_empty(),
        "끝난 forward 가 열린 표에 남았다"
    );
}

/// hook 사슬: 앞 hop 은 `last` 가 아니라 자리가 남고, 합이 문턱을 넘는 순간 링에 들며, 그
/// 뒤 hop 도 같은 줄에 붙는다.
#[test]
fn every_hop_of_a_chain_lands_on_the_same_row() {
    let log = SlowRequestLog::default();
    log.note_forwarded(9);
    log.finish_host(host(9, FAST, FAST));
    log.finish_plugin_hop(9, hop(50, FAST, HopOutcome::Ok), false);
    log.note_forwarded(9);
    log.finish_plugin_hop(9, hop(51, SLOW, HopOutcome::Ok), false);
    log.finish_plugin_hop(9, hop(52, FAST, HopOutcome::Error), true);
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1);
    let ids: Vec<u64> = rows[0]
        .plugin_hops
        .iter()
        .map(|h| h.host_request_id)
        .collect();
    assert_eq!(ids, [50, 51, 52]);
}

/// 열린 표는 상한에서 가장 오래 열린 것을 버린다 — 끝나지 않는 forward 가 표를 키우지
/// 못한다. 버려진 줄의 hop 이 나중에 오면 호스트 몫 없이 그 hop 으로만 판정한다.
#[test]
fn the_open_table_is_bounded_and_a_late_hop_stands_alone() {
    let log = SlowRequestLog::default();
    for seq in 1..=(OPEN_FORWARD_CAPACITY as u64 + 1) {
        log.note_forwarded(seq);
    }
    assert_eq!(log.lock().open.len(), OPEN_FORWARD_CAPACITY);
    log.finish_plugin_hop(1, hop(60, SLOW, HopOutcome::Ok), true);
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].host, None);
}
