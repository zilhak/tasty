//! 성공한 DB commit과 시도한 WAL checkpoint의 시간을 각각 집계한다.
//! tasty-telemetry가 이 크레이트에 의존하므로 통계를 여기서 정의해 순환 의존을 피한다.
//! 고정 크기 원자값을 Relaxed로 읽으므로 스냅샷 전체가 원자적이지는 않다.
//! 1마이크로초 미만도 횟수에는 포함하며, 관측이 없으면 평균은 None이다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 진단용 시간을 마이크로초로 변환한다.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

/// 저장소가 생성하고 Arc로 공유하는 지연 집계. 진단 조회는 저장소 mutex를 잠그지 않는다.
#[derive(Debug, Default)]
pub struct DbLatencyStats {
    /// 성공한 commit 수.
    commits: AtomicU64,
    /// 그 commit 들이 걸린 시간 합(마이크로초).
    commit_us_sum: AtomicU64,
    /// 그 최댓값(마이크로초).
    commit_us_max: AtomicU64,
    /// 시도한 checkpoint 수(busy 로 끝난 것 포함).
    checkpoints: AtomicU64,
    /// checkpoint 가 걸린 시간 합(마이크로초).
    checkpoint_us_sum: AtomicU64,
    /// 그 최댓값(마이크로초).
    checkpoint_us_max: AtomicU64,
    /// busy 때문에 완료하지 못한 checkpoint 수.
    checkpoints_busy: AtomicU64,
}

impl DbLatencyStats {
    /// 트랜잭션 하나가 commit 되는 데 걸린 시간. 호출부는 `tx.commit()` 이 `Ok` 로
    /// 돌아온 뒤에만 부른다.
    pub fn record_commit(&self, elapsed: Duration) {
        let us = as_micros(elapsed);
        self.commits.fetch_add(1, Ordering::Relaxed);
        self.commit_us_sum.fetch_add(us, Ordering::Relaxed);
        self.commit_us_max.fetch_max(us, Ordering::Relaxed);
    }

    /// checkpoint 한 번. `completed` 가 `false` 면 busy 로 끝난 것이고, 그때도 시간은
    /// 잰다 — 못 줄이고 돌아오는 데 걸린 시간도 그동안 붙잡힌 시간이다.
    pub fn record_checkpoint(&self, elapsed: Duration, completed: bool) {
        let us = as_micros(elapsed);
        self.checkpoints.fetch_add(1, Ordering::Relaxed);
        self.checkpoint_us_sum.fetch_add(us, Ordering::Relaxed);
        self.checkpoint_us_max.fetch_max(us, Ordering::Relaxed);
        if !completed {
            self.checkpoints_busy.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 누계를 읽는다. 필드 전체의 원자적 스냅샷은 아니다.
    pub fn snapshot(&self) -> DbLatencySnapshot {
        DbLatencySnapshot {
            commits: self.commits.load(Ordering::Relaxed),
            commit_us_sum: self.commit_us_sum.load(Ordering::Relaxed),
            commit_us_max: self.commit_us_max.load(Ordering::Relaxed),
            checkpoints: self.checkpoints.load(Ordering::Relaxed),
            checkpoint_us_sum: self.checkpoint_us_sum.load(Ordering::Relaxed),
            checkpoint_us_max: self.checkpoint_us_max.load(Ordering::Relaxed),
            checkpoints_busy: self.checkpoints_busy.load(Ordering::Relaxed),
        }
    }
}

/// [`DbLatencyStats`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DbLatencySnapshot {
    pub commits: u64,
    pub commit_us_sum: u64,
    pub commit_us_max: u64,
    pub checkpoints: u64,
    pub checkpoint_us_sum: u64,
    pub checkpoint_us_max: u64,
    pub checkpoints_busy: u64,
}

impl DbLatencySnapshot {
    /// 성공한 commit의 평균 시간(마이크로초). 관측이 없으면 None이다.
    pub fn commit_us_mean(&self) -> Option<u64> {
        (self.commits > 0).then(|| self.commit_us_sum / self.commits)
    }

    /// checkpoint 평균 시간(마이크로초). 관측이 없으면 None이다.
    pub fn checkpoint_us_mean(&self) -> Option<u64> {
        (self.checkpoints > 0).then(|| self.checkpoint_us_sum / self.checkpoints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_db_gauge_says_it_has_no_observation() {
        let s = DbLatencyStats::default().snapshot();
        assert_eq!(s.commit_us_mean(), None);
        assert_eq!(s.checkpoint_us_mean(), None);
        assert_eq!(s.checkpoints_busy, 0);
    }

    #[test]
    fn a_commit_faster_than_a_microsecond_still_counts() {
        let s = DbLatencyStats::default();
        s.record_commit(Duration::from_nanos(10));
        let snap = s.snapshot();
        assert_eq!(snap.commits, 1);
        assert_eq!(snap.commit_us_sum, 0);
        assert_eq!(snap.commit_us_mean(), Some(0));
    }

    #[test]
    fn the_largest_commit_is_kept_not_the_last() {
        let s = DbLatencyStats::default();
        s.record_commit(Duration::from_millis(9));
        s.record_commit(Duration::from_millis(1));
        assert_eq!(s.snapshot().commit_us_max, 9_000);
    }

    #[test]
    fn a_busy_checkpoint_is_timed_and_marked() {
        let s = DbLatencyStats::default();
        s.record_checkpoint(Duration::from_millis(2), false);
        s.record_checkpoint(Duration::from_millis(4), true);
        let snap = s.snapshot();
        assert_eq!(snap.checkpoints, 2);
        assert_eq!(snap.checkpoints_busy, 1);
        assert_eq!(snap.checkpoint_us_sum, 6_000);
        assert_eq!(snap.commits, 0);
    }
}
