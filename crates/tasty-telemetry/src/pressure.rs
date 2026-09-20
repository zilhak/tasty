//! 요청 압력 계측 — 큐 대기 · 큐 깊이 · handler 실행 시간.
//!
//! 이 모듈이 생기기 전 호스트가 IPC 경로에서 남기던 값은 `ipc_calls` 카운터 하나였고,
//! 요청이 **얼마나 밀렸는지 · 얼마나 걸렸는지**를 재는 자리는 한 곳도 없었다. 큐에서
//! 기다린 시간과 handler 안에서 보낸 시간이 구분되지 않으면, 느린 응답을 보고도 그것이
//! 적체인지 handler 비용인지 고를 수 없다. 이 집계가 그 둘을 다른 값으로 만든다.
//!
//! **왜 영구 기록이 아닌가**: 기존 `TelemetryEvent` 경로는 호출마다 저장소에 한 행을
//! 남긴다. 지연을 그렇게 재면 진단 자체가 호출당 기록 폭주를 다시 만든다. 그래서 여기
//! 값들은 프로세스 안에만 있고 크기가 고정이다 — 호출 수와 무관하게 원자값 여덟 개다.
//!
//! **왜 histogram 이 아닌가**: 분위수를 주려면 버킷 경계를 먼저 정해야 하는데, 그 경계는
//! 실제 분포를 보고 정할 일이다. 지금 이 집계가 답하는 것은 평균과 최대까지이고, 분위수는
//! 답하지 못한다. 그 한계를 값으로 남기려고 `count`·`sum`·`max` 를 따로 노출한다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 프로세스 수명 동안 누적되는 고정 크기 압력 집계.
///
/// 모든 갱신이 `Relaxed` 다 — 값들 사이에 순서 불변식이 없고 각각이 독립 카운터라,
/// 더 강한 순서를 요구하면 비용만 늘고 얻는 것이 없다. `snapshot` 이 여러 원자를 따로
/// 읽으므로 **한 스냅샷 안의 값들이 같은 순간의 것은 아니다**(예: `handler_calls` 를
/// 읽은 뒤 다른 스레드가 `handler_us_sum` 을 올릴 수 있다). 진단값이라 그 정도의
/// 어긋남은 허용하고, 대신 그 사실을 여기 적어 둔다.
#[derive(Debug, Default)]
pub struct PressureStats {
    /// 큐를 비운 횟수(= 비어 있지 않은 drain 만).
    queue_drains: AtomicU64,
    /// 한 번의 drain 이 집어 든 명령 수의 최댓값 = 관측된 최대 큐 깊이.
    queue_depth_max: AtomicU64,
    /// drain 으로 집어 든 명령 수의 합.
    queue_commands: AtomicU64,
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
}

/// 마이크로초로 접는다. 나노초를 그대로 더하면 `u64` 가 약 584 년에 넘치는데, 그보다
/// 실질적인 이유는 이 값들이 **사람이 읽는 진단값**이라 나노초 해상도가 의미가 없다는
/// 것이다. 1 마이크로초 미만은 0 으로 접힌다 — `count` 는 그대로 오르므로 "아주 빠른
/// 호출이 많았다" 와 "호출이 없었다" 는 구분된다.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

impl PressureStats {
    /// 큐를 한 번 비웠다 — 그때 집어 든 명령 수를 깊이로 기록한다.
    ///
    /// 빈 drain 은 호출하지 않는다(호출부가 비면 일찍 빠진다). 그래서 `queue_drains` 는
    /// "명령이 있었던 프레임 수" 이고, 프레임 수가 아니다.
    pub fn record_drain(&self, depth: usize) {
        let depth = depth as u64;
        self.queue_drains.fetch_add(1, Ordering::Relaxed);
        self.queue_commands.fetch_add(depth, Ordering::Relaxed);
        self.queue_depth_max.fetch_max(depth, Ordering::Relaxed);
    }

    /// 명령 하나가 큐에서 기다린 시간.
    pub fn record_queue_wait(&self, waited: Duration) {
        let us = as_micros(waited);
        self.queue_wait_us_sum.fetch_add(us, Ordering::Relaxed);
        self.queue_wait_us_max.fetch_max(us, Ordering::Relaxed);
    }

    /// handler 하나가 실행에 쓴 시간. 큐 대기는 포함하지 않는다 — 그것이 이 둘을
    /// 따로 재는 이유다.
    pub fn record_handler(&self, elapsed: Duration) {
        let us = as_micros(elapsed);
        self.handler_calls.fetch_add(1, Ordering::Relaxed);
        self.handler_us_sum.fetch_add(us, Ordering::Relaxed);
        self.handler_us_max.fetch_max(us, Ordering::Relaxed);
    }

    /// 지금까지의 누계를 한 덩어리로 읽는다. 위 struct 주석대로 **원자적 스냅샷이
    /// 아니다.**
    pub fn snapshot(&self) -> PressureSnapshot {
        PressureSnapshot {
            queue_drains: self.queue_drains.load(Ordering::Relaxed),
            queue_depth_max: self.queue_depth_max.load(Ordering::Relaxed),
            queue_commands: self.queue_commands.load(Ordering::Relaxed),
            queue_wait_us_sum: self.queue_wait_us_sum.load(Ordering::Relaxed),
            queue_wait_us_max: self.queue_wait_us_max.load(Ordering::Relaxed),
            handler_calls: self.handler_calls.load(Ordering::Relaxed),
            handler_us_sum: self.handler_us_sum.load(Ordering::Relaxed),
            handler_us_max: self.handler_us_max.load(Ordering::Relaxed),
        }
    }
}

/// [`PressureStats`] 의 한 시점 읽기. 평균은 파생값이라 필드로 두지 않고 메서드로 낸다 —
/// 분모가 0 인 경우를 소비자마다 다르게 처리하지 않게 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PressureSnapshot {
    pub queue_drains: u64,
    pub queue_depth_max: u64,
    pub queue_commands: u64,
    pub queue_wait_us_sum: u64,
    pub queue_wait_us_max: u64,
    pub handler_calls: u64,
    pub handler_us_sum: u64,
    pub handler_us_max: u64,
}

impl PressureSnapshot {
    /// 명령 하나가 큐에서 기다린 평균(마이크로초). 관측이 없으면 `None` — 0 을 돌려주면
    /// "기다림이 없었다" 와 "잰 적이 없다" 가 같은 값이 된다.
    pub fn queue_wait_us_mean(&self) -> Option<u64> {
        (self.queue_commands > 0).then(|| self.queue_wait_us_sum / self.queue_commands)
    }

    /// handler 하나의 평균 실행 시간(마이크로초). 위와 같은 이유로 `Option`.
    pub fn handler_us_mean(&self) -> Option<u64> {
        (self.handler_calls > 0).then(|| self.handler_us_sum / self.handler_calls)
    }

    /// drain 한 번이 집어 든 평균 명령 수. 이 값이 1 에 가까우면 적체가 없는 것이고,
    /// 크면 한 프레임이 여러 요청을 몰아 처리하고 있다는 뜻이다.
    pub fn queue_depth_mean(&self) -> Option<u64> {
        (self.queue_drains > 0).then(|| self.queue_commands / self.queue_drains)
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

    // 1 마이크로초 미만은 0 으로 접히지만 **횟수는 오른다** — 그래야 "아주 빠른 호출이
    // 많았다" 가 "호출이 없었다" 로 보이지 않는다.
    #[test]
    fn a_sub_microsecond_call_still_counts() {
        let p = PressureStats::default();
        p.record_handler(Duration::from_nanos(10));
        let s = p.snapshot();
        assert_eq!(s.handler_calls, 1);
        assert_eq!(s.handler_us_sum, 0);
        assert_eq!(s.handler_us_mean(), Some(0), "0 이지 None 이 아니다");
    }
}
