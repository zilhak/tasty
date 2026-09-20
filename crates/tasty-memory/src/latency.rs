//! DB 지연 계측 — 트랜잭션 commit 과 WAL checkpoint 가 걸린 시간.
//!
//! 요청이 느릴 때 원인 후보는 넷이다: 호스트 큐에서 기다렸거나, handler 가 오래
//! 걸렸거나, plugin 이 늦게 답했거나, **디스크가 늦게 받아줬거나.** 앞의 셋은
//! `tasty-telemetry` 의 `PressureStats`·`PluginWaitStats` 가 잰다. 이 모듈이 넷째를
//! 잰다 — 그 셋과 같은 축에 두면 운영자가 "handler 가 느리다" 와 "handler 안의
//! commit 이 느리다" 를 못 가른다.
//!
//! **왜 여기에 사는가**: 값의 성질은 `PressureStats` 와 같지만 자리가 다르다.
//! `tasty-telemetry` 가 `tasty-memory` 를 의존하므로 반대 방향 의존을 만들 수 없고,
//! 그래서 같은 모양의 타입을 이 크레이트 안에 둔다. 공유하는 성질은 `pressure.rs`
//! 와 같다: 프로세스 수명 누계 · 고정 크기(원자값 일곱) · `Relaxed` · 스냅샷이
//! 원자적이지 않음 · 1 마이크로초 미만은 0 으로 접히되 횟수는 오름 · 평균은 파생값
//! 이라 관측이 없으면 `None`.
//!
//! **왜 성공한 commit 만 세는가**: 기록은 `tx.commit()` 이 `Ok` 로 돌아온 뒤에
//! 일어난다. 실패한 commit 은 롤백이라 디스크에 남긴 것이 없고, 그 시간을 성공분과
//! 같은 합에 넣으면 평균이 "쓰기 한 건이 걸리는 시간" 을 더는 뜻하지 않는다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 마이크로초로 접는다 — `tasty-telemetry` 의 같은 이름 함수와 같은 이유다(사람이
/// 읽는 진단값이라 나노초 해상도가 의미가 없고, `u64` 나노초는 넘칠 수 있다).
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

/// 프로세스 수명 동안 누적되는 고정 크기 DB 지연 집계.
///
/// `MemoryStore` 가 열릴 때 함께 태어나고 `Arc` 로 밖에 나간다. 호스트는 그 `Arc` 를
/// 들고 읽으므로 **진단이 메모리 뮤텍스를 안 잡는다** — 적체를 재려고 적체하는
/// 자물쇠를 잡으면 진단이 같이 막힌다.
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
    /// 그중 **끝까지 못 간** 횟수(다른 커넥션이 읽는 중이라 `busy=1`). 이 값이 크면
    /// WAL 이 안 줄어드는 이유가 지연이 아니라 경합이다 — 시간만 봐서는 안 갈린다.
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

    /// 지금까지의 누계를 한 덩어리로 읽는다. 위 struct 주석대로 **원자적 스냅샷이
    /// 아니다.**
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
    /// commit 하나의 평균(마이크로초). 관측이 없으면 `None` — 0 을 돌려주면
    /// "즉시 끝났다" 와 "잰 적이 없다" 가 같은 값이 된다.
    pub fn commit_us_mean(&self) -> Option<u64> {
        (self.commits > 0).then(|| self.commit_us_sum / self.commits)
    }

    /// checkpoint 하나의 평균(마이크로초). 위와 같은 이유로 `Option`.
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
        // 시간은 0 으로 접히지만 횟수는 올라야 "빠른 commit 이 있었다" 와 "없었다" 가
        // 갈린다.
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
        // commit 과 섞이지 않는다 — 두 모수는 다른 자리에서 온다.
        assert_eq!(snap.commits, 0);
    }
}
