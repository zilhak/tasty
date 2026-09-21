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
        outcome: HostOutcomeCell::default(),
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

/// plugin hop 쪽 판정도 문턱 **이상**이다 — 호스트 몫과 hop 대기의 합이 문턱과 같으면 들고,
/// 1µs 모자라면 안 든다.
#[test]
fn a_forward_is_kept_at_the_threshold_and_not_just_under_it() {
    let log = SlowRequestLog::default();
    for (seq, wait) in [
        (1, SLOW_REQUEST_THRESHOLD - Duration::from_micros(1)),
        (2, SLOW_REQUEST_THRESHOLD),
    ] {
        log.note_forwarded(seq);
        log.finish_host(host(seq, Duration::ZERO, Duration::ZERO));
        log.finish_plugin_hop(seq, hop(seq, wait, HopOutcome::Ok), true);
    }
    assert_eq!(seqs(&log), [2]);
}

/// 열린 자리를 잃은 사슬이라도 앞 hop 이 `last` 가 아니면 새 자리를 열어 뒤 hop 과 잇는다.
/// 그렇게 여는 자리도 열린 표의 상한을 지킨다.
#[test]
fn a_late_hop_that_is_not_last_opens_a_bounded_entry_for_the_next() {
    let log = SlowRequestLog::default();
    log.finish_plugin_hop(1, hop(70, FAST, HopOutcome::Ok), false);
    log.finish_plugin_hop(1, hop(71, SLOW, HopOutcome::Ok), true);
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1);
    let ids: Vec<u64> = rows[0]
        .plugin_hops
        .iter()
        .map(|h| h.host_request_id)
        .collect();
    assert_eq!(ids, [70, 71]);

    for seq in 10..=(10 + OPEN_FORWARD_CAPACITY as u64) {
        log.finish_plugin_hop(seq, hop(seq, FAST, HopOutcome::Ok), false);
    }
    assert_eq!(log.lock().open.len(), OPEN_FORWARD_CAPACITY);
}

/// 한 줄의 hop 수는 [`MAX_PLUGIN_HOPS`] 에서 멈춘다 — pre-hook · target · post-hook 이 사슬의
/// 최대이고, 그 밖의 hop 이 줄을 키우지 못한다.
#[test]
fn a_row_keeps_at_most_the_hops_of_one_chain() {
    let log = SlowRequestLog::default();
    log.finish_host(host(3, SLOW, FAST));
    for req_id in 0..(MAX_PLUGIN_HOPS as u64 + 2) {
        log.finish_plugin_hop(3, hop(req_id, FAST, HopOutcome::Ok), false);
    }
    assert_eq!(log.snapshot().rows[0].plugin_hops.len(), MAX_PLUGIN_HOPS);
}

/// 메서드 칸은 호출자 문자열이라 길이를 자른다 — 상한을 넘는 이름은 상한 안의 마지막 char 경계까지만
/// 실리고(여러 바이트 글자를 반으로 가르지 않는다), 상한 이하 이름은 그대로다.
#[test]
fn a_method_name_past_the_cap_is_cut_at_a_char_boundary() {
    let log = SlowRequestLog::default();
    let ascii = "x".repeat(MAX_METHOD_BYTES + 40);
    // 3 바이트 글자로 채워 상한이 글자 한가운데 떨어지게 한다.
    let wide = "가".repeat(MAX_METHOD_BYTES);
    let exact = "y".repeat(MAX_METHOD_BYTES);
    for (seq, method) in [(1, ascii.as_str()), (2, wide.as_str()), (3, exact.as_str())] {
        log.finish_host(HostLeg {
            request_seq: seq,
            method,
            caller: CallerKind::Agent,
            queue_wait: SLOW,
            host: FAST,
            outcome: HostOutcomeCell::default(),
        });
    }
    let rows = log.snapshot().rows;
    let method = |i: usize| rows[i].host.as_ref().expect("host part").method.clone();
    assert_eq!(method(0), "x".repeat(MAX_METHOD_BYTES));
    let cut = method(1);
    assert!(cut.len() <= MAX_METHOD_BYTES, "{}", cut.len());
    assert!(cut.len() > MAX_METHOD_BYTES - 3, "{}", cut.len());
    assert!(cut.chars().all(|c| c == '가'));
    assert_eq!(method(2), exact);
}
