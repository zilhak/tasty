//! 큐 대기·handler 실행·plugin 응답 대기·연결 수를 각각 집계한다.
//! 고정 크기 카운터와 히스토그램을 메모리에만 유지하므로 요청마다 파일을 쓰지 않는다.
//! 평균·최대와 별도로 분포를 제공하되, 정확한 분위수 대신 고정 구간별 개수를 보여 준다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 지연 구간의 상한(마이크로초). 같은 값은 해당 구간에 포함한다.
/// 약 3.16배 간격으로 10µs~1초를 나누고, 1초를 넘으면 마지막 구간에 넣는다.
/// 마지막 구간에는 상한이 없으므로 실제 최댓값은 max를 함께 본다.
/// 선택 근거: docs/architecture/ipc-server.md#연결-수와-시간-분포.
pub const LATENCY_BUCKET_BOUNDS_US: [u64; 11] = [
    10, 32, 100, 316, 1_000, 3_162, 10_000, 31_623, 100_000, 316_228, 1_000_000,
];

/// 버킷 수 = 유한 상한 수 + 넘침 한 칸.
pub const LATENCY_BUCKET_COUNT: usize = LATENCY_BUCKET_BOUNDS_US.len() + 1;

/// 관측값이 속한 구간을 찾는다.
fn bucket_of(us: u64) -> usize {
    LATENCY_BUCKET_BOUNDS_US
        .iter()
        .position(|&bound| us <= bound)
        .unwrap_or(LATENCY_BUCKET_BOUNDS_US.len())
}

/// 프로세스 수명 동안 누적하는 고정 크기 히스토그램.
/// Relaxed 카운터를 따로 읽으므로 스냅샷의 구간 합과 다른 count가 잠깐 다를 수 있다.
#[derive(Debug, Default)]
pub struct LatencyHistogram {
    counts: [AtomicU64; LATENCY_BUCKET_COUNT],
}

impl LatencyHistogram {
    /// 관측값을 기록한다. 1마이크로초 미만은 0으로 환산해 첫 구간에 넣는다.
    pub fn record_us(&self, us: u64) {
        self.counts[bucket_of(us)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> HistogramSnapshot {
        let mut counts = [0u64; LATENCY_BUCKET_COUNT];
        for (dst, src) in counts.iter_mut().zip(self.counts.iter()) {
            *dst = src.load(Ordering::Relaxed);
        }
        HistogramSnapshot { counts }
    }
}

/// [`LatencyHistogram`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct HistogramSnapshot {
    /// 버킷마다의 관측 수. **누적이 아니다** — 칸끼리 겹치지 않고 합이 전체 관측 수다
    /// (Prometheus 의 `le` 누적 버킷과 다르다). 마지막 칸은 마지막 상한을 넘은 것들이다.
    pub counts: [u64; LATENCY_BUCKET_COUNT],
}

impl HistogramSnapshot {
    /// 소비자가 상수를 복제하지 않도록 스냅샷과 함께 제공하는 구간 상한.
    pub fn bounds_us() -> &'static [u64; LATENCY_BUCKET_BOUNDS_US.len()] {
        &LATENCY_BUCKET_BOUNDS_US
    }

    /// 모든 구간의 관측 수 합.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }
}

/// 프로세스 수명 동안 누적하는 고정 크기 집계.
/// 독립 카운터를 Relaxed로 갱신한다. 스냅샷의 여러 값이 같은 순간을 나타내지는 않는다.
#[derive(Debug, Default)]
pub struct PressureStats {
    /// 큐를 비운 횟수(= 비어 있지 않은 drain 만).
    queue_drains: AtomicU64,
    /// 한 번의 drain 이 집어 든 명령 수의 최댓값 = 관측된 최대 큐 깊이.
    queue_depth_max: AtomicU64,
    /// drain 으로 집어 든 명령 수의 합. **회차가 끝날 때** 오른다.
    queue_commands: AtomicU64,
    /// 큐 대기를 기록한 명령 수이며 평균의 분모다. 회차가 끝나야 오르는 queue_commands를
    /// 쓰면 진행 중인 회차의 대기 시간만 합계에 먼저 반영될 수 있다.
    queue_waits: AtomicU64,
    /// 큐에 들어간 뒤 꺼내질 때까지의 대기 시간 합(마이크로초).
    queue_wait_us_sum: AtomicU64,
    /// 그 대기 시간의 최댓값(마이크로초).
    queue_wait_us_max: AtomicU64,
    /// handler 를 실행한 횟수.
    handler_calls: AtomicU64,
    /// handler 실행 시간 합(마이크로초).
    handler_us_sum: AtomicU64,
    /// handler 실행 시간 최댓값(마이크로초).
    handler_us_max: AtomicU64,
    /// 큐 대기 시간의 분포.
    queue_wait_hist: LatencyHistogram,
    /// 큐 대기와 별도로 집계하는 handler 실행 시간의 분포.
    handler_hist: LatencyHistogram,
}

/// 진단값을 마이크로초로 환산한다. 1마이크로초 미만은 0이지만 관측 횟수는 증가한다.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

impl PressureStats {
    /// 비어 있지 않은 drain이 꺼낸 명령 수를 기록한다. 빈 drain은 호출자가 제외한다.
    pub fn record_drain(&self, depth: usize) {
        let depth = depth as u64;
        self.queue_drains.fetch_add(1, Ordering::Relaxed);
        self.queue_commands.fetch_add(depth, Ordering::Relaxed);
        self.queue_depth_max.fetch_max(depth, Ordering::Relaxed);
    }

    /// 명령 하나가 큐에서 기다린 시간.
    pub fn record_queue_wait(&self, waited: Duration) {
        let us = as_micros(waited);
        self.queue_waits.fetch_add(1, Ordering::Relaxed);
        self.queue_wait_us_sum.fetch_add(us, Ordering::Relaxed);
        self.queue_wait_us_max.fetch_max(us, Ordering::Relaxed);
        self.queue_wait_hist.record_us(us);
    }

    /// handler 실행 시간을 기록한다. 큐 대기는 제외한다.
    pub fn record_handler(&self, elapsed: Duration) {
        let us = as_micros(elapsed);
        self.handler_calls.fetch_add(1, Ordering::Relaxed);
        self.handler_us_sum.fetch_add(us, Ordering::Relaxed);
        self.handler_us_max.fetch_max(us, Ordering::Relaxed);
        self.handler_hist.record_us(us);
    }

    /// 카운터를 읽는다. 여러 값이 동시에 고정된 스냅샷은 아니다.
    pub fn snapshot(&self) -> PressureSnapshot {
        PressureSnapshot {
            queue_drains: self.queue_drains.load(Ordering::Relaxed),
            queue_depth_max: self.queue_depth_max.load(Ordering::Relaxed),
            queue_commands: self.queue_commands.load(Ordering::Relaxed),
            queue_waits: self.queue_waits.load(Ordering::Relaxed),
            queue_wait_us_sum: self.queue_wait_us_sum.load(Ordering::Relaxed),
            queue_wait_us_max: self.queue_wait_us_max.load(Ordering::Relaxed),
            handler_calls: self.handler_calls.load(Ordering::Relaxed),
            handler_us_sum: self.handler_us_sum.load(Ordering::Relaxed),
            handler_us_max: self.handler_us_max.load(Ordering::Relaxed),
            queue_wait_hist: self.queue_wait_hist.snapshot(),
            handler_hist: self.handler_hist.snapshot(),
        }
    }
}

/// 집계 스냅샷. 평균과 관측 부재 처리를 공통 메서드로 제공한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PressureSnapshot {
    pub queue_drains: u64,
    pub queue_depth_max: u64,
    pub queue_commands: u64,
    pub queue_waits: u64,
    pub queue_wait_us_sum: u64,
    pub queue_wait_us_max: u64,
    pub handler_calls: u64,
    pub handler_us_sum: u64,
    pub handler_us_max: u64,
    pub queue_wait_hist: HistogramSnapshot,
    pub handler_hist: HistogramSnapshot,
}

impl PressureSnapshot {
    /// 명령의 평균 큐 대기 시간(마이크로초). 대기 기록 수로 나누며 관측이 없으면 None이다.
    /// 각 카운터는 따로 읽으므로 동시에 갱신되는 값 사이의 일치는 보장하지 않는다.
    pub fn queue_wait_us_mean(&self) -> Option<u64> {
        (self.queue_waits > 0).then(|| self.queue_wait_us_sum / self.queue_waits)
    }

    /// handler 하나의 평균 실행 시간(마이크로초). 위와 같은 이유로 `Option`.
    pub fn handler_us_mean(&self) -> Option<u64> {
        (self.handler_calls > 0).then(|| self.handler_us_sum / self.handler_calls)
    }

    /// 비어 있지 않은 drain 한 번이 꺼낸 평균 명령 수. 값이 1이어도 대기 시간이 짧다는 뜻은 아니다.
    pub fn queue_depth_mean(&self) -> Option<u64> {
        (self.queue_drains > 0).then(|| self.queue_commands / self.queue_drains)
    }
}

/// 응답과 매칭된 host→plugin 요청의 대기 시간을 집계한다.
/// 호스트 큐·handler 시간과 따로 기록하며 응답이 오지 않은 요청은 포함하지 않는다.
/// tasty-host-plugin의 응답 매칭부가 갱신하므로 Core의 PressureStats와 분리한다.
/// 프로세스 수명 누계이며 고정 크기·Relaxed 갱신·비원자적 스냅샷을 사용한다.
#[derive(Debug, Default)]
pub struct PluginWaitStats {
    /// 응답이 **매칭된** 요청 수. 응답이 영영 안 온 요청은 여기 안 센다.
    matched: AtomicU64,
    /// 그 왕복 대기 시간 합(마이크로초).
    us_sum: AtomicU64,
    /// 그 최댓값(마이크로초).
    us_max: AtomicU64,
    /// plugin 응답 대기 시간의 분포. 분포만으로 느린 원인을 확정하지는 않는다.
    hist: LatencyHistogram,
}

impl PluginWaitStats {
    /// 요청 하나의 응답이 도착했다 — 보낸 뒤 흐른 시간을 접는다.
    pub fn record(&self, waited: Duration) {
        let us = as_micros(waited);
        self.matched.fetch_add(1, Ordering::Relaxed);
        self.us_sum.fetch_add(us, Ordering::Relaxed);
        self.us_max.fetch_max(us, Ordering::Relaxed);
        self.hist.record_us(us);
    }

    pub fn snapshot(&self) -> PluginWaitSnapshot {
        PluginWaitSnapshot {
            matched: self.matched.load(Ordering::Relaxed),
            us_sum: self.us_sum.load(Ordering::Relaxed),
            us_max: self.us_max.load(Ordering::Relaxed),
            hist: self.hist.snapshot(),
        }
    }
}

/// [`PluginWaitStats`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PluginWaitSnapshot {
    pub matched: u64,
    pub us_sum: u64,
    pub us_max: u64,
    pub hist: HistogramSnapshot,
}

impl PluginWaitSnapshot {
    /// 왕복 하나의 평균(마이크로초). 관측이 없으면 `None` — 위와 같은 이유다.
    pub fn us_mean(&self) -> Option<u64> {
        (self.matched > 0).then(|| self.us_sum / self.matched)
    }
}

/// 동시에 살아 있는 IPC 연결 수. accept 루프가 자리를 배정하고 연결의 Drop에서 반납한다.
/// 연결 단계의 거절은 요청별 시간 집계에 포함되지 않는다.
/// 상한 정책은 서버가 try_open에 전달하며 이 타입에는 복제하지 않는다.
#[derive(Debug, Default)]
pub struct ConnectionStats {
    /// 현재 연결 수. 연결을 닫으면 감소한다.
    live: AtomicU64,
    /// 관측한 최대 동시 연결 수.
    live_max: AtomicU64,
    /// 자리를 받아 간 연결 수의 누계.
    accepted: AtomicU64,
    /// 상한에 걸려 거절된 연결 수의 누계.
    refused_saturated: AtomicU64,
    /// accept 대기 상한을 기록한 연결 수 — accept 루프가 꺼낸 TCP 연결 **전부**(자리를 받은 것과
    /// 거절된 것 둘 다)다. 그래서 `accepted + refused_saturated` 와 같다.
    accept_waits: AtomicU64,
    /// 연결 하나가 OS 의 accept 큐에서 기다렸을 수 있는 시간의 **상한** 합(마이크로초). 재는 법은
    /// [`ConnectionStats::record_accept_wait`].
    accept_wait_bound_us_sum: AtomicU64,
    /// 그 상한의 최댓값(마이크로초).
    accept_wait_bound_us_max: AtomicU64,
}

impl ConnectionStats {
    /// 원자적으로 수를 올려 자리를 예약하고, 상한을 넘으면 되돌린 뒤 None을 반환한다.
    /// 성공하면 예약 직후 연결 수를 반환한다. 조회 후 별도로 증가시키면 마지막 자리가 중복 배정될 수 있다.
    pub fn try_open(&self, limit: u64) -> Option<u64> {
        let prev = self.live.fetch_add(1, Ordering::Relaxed);
        if prev >= limit {
            self.live.fetch_sub(1, Ordering::Relaxed);
            self.refused_saturated.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let now = prev + 1;
        self.accepted.fetch_add(1, Ordering::Relaxed);
        self.live_max.fetch_max(now, Ordering::Relaxed);
        Some(now)
    }

    /// 연결을 꺼낸 시각과 마지막 WouldBlock 시각의 차이를 accept 대기의 상한으로 기록한다.
    /// 실제 도착 시각은 알 수 없으므로 정확한 대기 시간이 아니다.
    pub fn record_accept_wait(&self, bound: Duration) {
        let us = as_micros(bound);
        self.accept_waits.fetch_add(1, Ordering::Relaxed);
        self.accept_wait_bound_us_sum
            .fetch_add(us, Ordering::Relaxed);
        self.accept_wait_bound_us_max
            .fetch_max(us, Ordering::Relaxed);
    }

    /// try_open이 성공한 자리 하나를 반납한다. 중복 반납하면 live가 언더플로한다.
    pub fn close(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ConnectionSnapshot {
        ConnectionSnapshot {
            live: self.live.load(Ordering::Relaxed),
            live_max: self.live_max.load(Ordering::Relaxed),
            accepted: self.accepted.load(Ordering::Relaxed),
            refused_saturated: self.refused_saturated.load(Ordering::Relaxed),
            accept_waits: self.accept_waits.load(Ordering::Relaxed),
            accept_wait_bound_us_sum: self.accept_wait_bound_us_sum.load(Ordering::Relaxed),
            accept_wait_bound_us_max: self.accept_wait_bound_us_max.load(Ordering::Relaxed),
        }
    }
}

/// 연결 집계 스냅샷. 서버가 가진 상한은 응답 구성 시 함께 제공한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ConnectionSnapshot {
    pub live: u64,
    pub live_max: u64,
    pub accepted: u64,
    pub refused_saturated: u64,
    pub accept_waits: u64,
    pub accept_wait_bound_us_sum: u64,
    pub accept_wait_bound_us_max: u64,
}

impl ConnectionSnapshot {
    /// accept 대기 상한의 평균(마이크로초). 기록이 없으면 `None`.
    pub fn accept_wait_bound_us_mean(&self) -> Option<u64> {
        (self.accept_waits > 0).then(|| self.accept_wait_bound_us_sum / self.accept_waits)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_aggregate_says_it_has_no_observation() {
        let s = PressureStats::default().snapshot();
        assert_eq!(s.queue_wait_us_mean(), None);
        assert_eq!(s.handler_us_mean(), None);
        assert_eq!(s.queue_depth_mean(), None);
    }

    #[test]
    fn a_drain_records_its_depth_and_keeps_the_largest() {
        let p = PressureStats::default();
        p.record_drain(3);
        p.record_drain(11);
        p.record_drain(1);
        let s = p.snapshot();
        assert_eq!(s.queue_drains, 3);
        assert_eq!(s.queue_commands, 15);
        assert_eq!(s.queue_depth_max, 11, "최댓값은 내려가지 않는다");
        assert_eq!(s.queue_depth_mean(), Some(5));
    }

    /// 회차가 아직 안 끝났어도(명령을 꺼내 대기는 기록했지만 `record_drain` 전) 평균은 최댓값을
    /// 안 넘고, 분포의 합이 평균의 분모와 같다 — 조회는 늘 자기 회차 안에서 스냅샷을 찍는다.
    #[test]
    fn the_wait_mean_shares_its_modulus_with_the_sum_while_a_round_is_open() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_micros(300));
        p.record_queue_wait(Duration::from_micros(100));
        let s = p.snapshot();
        assert_eq!(s.queue_commands, 0, "대조군: 회차가 아직 안 끝났다");
        assert_eq!(s.queue_waits, 2);
        assert_eq!(s.queue_wait_us_mean(), Some(200));
        assert!(s.queue_wait_us_mean().unwrap() <= s.queue_wait_us_max);
        assert_eq!(s.queue_wait_hist.total(), s.queue_waits);
    }

    #[test]
    fn queue_wait_and_handler_time_are_separate_values() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_millis(40));
        p.record_drain(1);
        p.record_handler(Duration::from_millis(2));
        let s = p.snapshot();
        assert_eq!(s.queue_wait_us_max, 40_000);
        assert_eq!(s.handler_us_max, 2_000);
        assert_eq!(
            s.queue_wait_us_mean(),
            Some(40_000),
            "큐 대기가 handler 시간에 섞이면 안 된다"
        );
        assert_eq!(s.handler_us_mean(), Some(2_000));
    }

    #[test]
    fn a_sub_microsecond_call_still_counts() {
        let p = PressureStats::default();
        p.record_handler(Duration::from_nanos(10));
        let s = p.snapshot();
        assert_eq!(s.handler_calls, 1);
        assert_eq!(s.handler_us_sum, 0);
        assert_eq!(s.handler_us_mean(), Some(0), "0 이지 None 이 아니다");
    }

    #[test]
    fn an_untouched_plugin_wait_says_it_has_no_observation() {
        let s = PluginWaitStats::default().snapshot();
        assert_eq!(s.matched, 0);
        assert_eq!(s.us_mean(), None);
    }

    #[test]
    fn plugin_wait_keeps_the_largest_round_trip() {
        let p = PluginWaitStats::default();
        p.record(Duration::from_millis(3));
        p.record(Duration::from_millis(21));
        p.record(Duration::from_millis(6));
        let s = p.snapshot();
        assert_eq!(s.matched, 3);
        assert_eq!(s.us_sum, 30_000);
        assert_eq!(s.us_max, 21_000, "최댓값은 내려가지 않는다");
        assert_eq!(s.us_mean(), Some(10_000));
    }

    #[test]
    fn a_plugin_round_trip_is_not_host_handler_time() {
        let host = PressureStats::default();
        let plugin = PluginWaitStats::default();
        host.record_handler(Duration::from_millis(2));
        plugin.record(Duration::from_millis(900));
        assert_eq!(host.snapshot().handler_us_max, 2_000);
        assert_eq!(plugin.snapshot().us_max, 900_000);
    }

    /// accept 대기 상한은 자리 계수와 따로 쌓이고, 평균은 자기 기록 수로 나눈다.
    #[test]
    fn the_accept_wait_bound_has_its_own_count() {
        let c = ConnectionStats::default();
        assert_eq!(c.snapshot().accept_wait_bound_us_mean(), None);
        c.record_accept_wait(Duration::from_millis(100));
        c.record_accept_wait(Duration::from_millis(20));
        let s = c.snapshot();
        assert_eq!(
            (
                s.accept_waits,
                s.accept_wait_bound_us_sum,
                s.accept_wait_bound_us_max
            ),
            (2, 120_000, 100_000)
        );
        assert_eq!(s.accept_wait_bound_us_mean(), Some(60_000));
        assert_eq!((s.live, s.accepted), (0, 0), "기록은 자리를 잡지 않는다");
    }

    #[test]
    fn a_closed_connection_frees_the_seat_but_not_the_total() {
        let c = ConnectionStats::default();
        assert!(c.try_open(4).is_some());
        assert!(c.try_open(4).is_some());
        c.close();
        let s = c.snapshot();
        assert_eq!(s.live, 1, "하나 닫혔으니 자리는 하나 남았다");
        assert_eq!(s.live_max, 2, "최댓값은 내려가지 않는다");
        assert_eq!(s.accepted, 2, "누계는 닫아도 안 내려간다");
        assert_eq!(s.refused_saturated, 0);
    }

    // 거절 후 연결 수가 복원되고 거절 누계만 증가해야 한다.
    #[test]
    fn a_refusal_counts_itself_without_taking_a_seat() {
        let c = ConnectionStats::default();
        let _held: Vec<_> = (0..2)
            .map(|_| c.try_open(2).expect("상한까지는 열린다"))
            .collect();
        assert!(c.try_open(2).is_none(), "상한을 넘으면 거절이다");
        assert!(c.try_open(2).is_none());
        let s = c.snapshot();
        assert_eq!(s.live, 2, "거절이 자리를 먹으면 안 된다");
        assert_eq!(s.accepted, 2, "거절은 accepted 에 안 센다");
        assert_eq!(s.refused_saturated, 2);
        assert_eq!(s.live_max, 2);
    }

    #[test]
    fn a_returned_seat_lets_the_next_connection_in() {
        let c = ConnectionStats::default();
        assert_eq!(c.try_open(1), Some(1));
        assert!(c.try_open(1).is_none());
        c.close();
        assert_eq!(c.try_open(1), Some(1), "반납된 자리로 다음이 들어와야 한다");
        assert_eq!(c.snapshot().refused_saturated, 1);
    }

    // 연결 포화와 요청 처리 시간은 별도로 집계한다.
    #[test]
    fn a_full_connection_table_is_not_a_slow_handler() {
        let host = PressureStats::default();
        let conn = ConnectionStats::default();
        assert!(conn.try_open(1).is_some());
        assert!(conn.try_open(1).is_none());
        assert_eq!(
            host.snapshot().handler_calls,
            0,
            "요청은 하나도 안 들어왔다"
        );
        assert_eq!(conn.snapshot().refused_saturated, 1, "그래도 자리는 찼다");
    }

    #[test]
    fn an_observation_equal_to_a_bound_lands_in_that_bucket() {
        let h = LatencyHistogram::default();
        h.record_us(10);
        h.record_us(11);
        let c = h.snapshot().counts;
        assert_eq!(c[0], 1, "10 µs 는 상한이 10 인 첫 칸이다");
        assert_eq!(c[1], 1, "11 µs 는 다음 칸이다");
    }

    #[test]
    fn everything_past_the_last_bound_lands_in_the_overflow_bucket() {
        let h = LatencyHistogram::default();
        h.record_us(LATENCY_BUCKET_BOUNDS_US[LATENCY_BUCKET_BOUNDS_US.len() - 1] + 1);
        h.record_us(u64::MAX);
        let s = h.snapshot();
        assert_eq!(s.counts[LATENCY_BUCKET_COUNT - 1], 2);
        assert_eq!(s.total(), 2);
        assert_eq!(
            HistogramSnapshot::bounds_us().len(),
            LATENCY_BUCKET_COUNT - 1,
            "상한 수보다 칸이 하나 많다 — 그 하나가 넘침이다"
        );
    }

    #[test]
    fn the_buckets_do_not_overlap() {
        let h = LatencyHistogram::default();
        for us in [1, 50, 5_000, 500_000] {
            h.record_us(us);
        }
        let s = h.snapshot();
        assert_eq!(s.total(), 4, "합이 관측 수여야 한다");
        assert_eq!(s.counts.iter().filter(|&&n| n == 1).count(), 4);
    }

    // 평균·최대·횟수가 같아도 분포는 다를 수 있다.
    #[test]
    fn two_runs_with_the_same_mean_have_different_shapes() {
        // 꼬리형: 한 건이 1 s, 나머지 셋이 0 — "대부분 빠른데 몇 건이 튄다".
        let tail = PressureStats::default();
        for us in [1_000_000, 0, 0, 0] {
            tail.record_handler(Duration::from_micros(us));
        }
        // 고른형: 넷이 전부 250 ms — "전부 조금씩 느리다".
        let spread = PressureStats::default();
        for _ in 0..4 {
            spread.record_handler(Duration::from_micros(250_000));
        }

        let t = tail.snapshot();
        let p = spread.snapshot();
        assert_eq!(t.handler_us_sum, p.handler_us_sum, "합이 같다");
        assert_eq!(t.handler_calls, p.handler_calls, "건수가 같다");
        assert_eq!(t.handler_us_mean(), p.handler_us_mean(), "평균이 같다");
        assert_ne!(
            t.handler_hist, p.handler_hist,
            "평균이 같아도 분포는 달라야 한다 — 그것이 이 값의 존재 이유다"
        );
        assert_eq!(t.handler_hist.counts[0], 3, "꼬리형은 셋이 맨 앞 칸이다");
        assert_eq!(p.handler_hist.counts[0], 0, "고른형은 맨 앞 칸이 비어 있다");
    }

    #[test]
    fn the_two_time_moduli_keep_separate_distributions() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_millis(40));
        p.record_handler(Duration::from_micros(2));
        let s = p.snapshot();
        assert_eq!(s.queue_wait_hist.total(), 1);
        assert_eq!(s.handler_hist.total(), 1);
        assert_eq!(s.handler_hist.counts[0], 1, "2 µs 는 맨 앞 칸");
        assert_eq!(
            s.queue_wait_hist.counts[0], 0,
            "40 ms 가 앞 칸에 오면 안 된다"
        );
        assert_ne!(s.queue_wait_hist, s.handler_hist);
    }

    #[test]
    fn a_plugin_round_trip_has_its_own_distribution() {
        let host = PressureStats::default();
        let plugin = PluginWaitStats::default();
        host.record_handler(Duration::from_millis(2));
        plugin.record(Duration::from_millis(900));
        assert_eq!(plugin.snapshot().hist.total(), 1);
        assert_eq!(
            host.snapshot().handler_hist.total(),
            1,
            "호스트 칸에 plugin 왕복이 섞이면 안 된다"
        );
        assert_ne!(plugin.snapshot().hist, host.snapshot().handler_hist);
    }
}
