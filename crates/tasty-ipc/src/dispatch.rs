//! IPC 명령을 **큐에서 꺼낸 쪽**의 누계 — dispatch 회차가 어디서 멈췄는가, 그리고 기한이 지나
//! 실행하지 않은 명령이 몇인가.
//!
//! 큐에 든 쪽은 입장 장부([`crate::admission::CommandAdmission`])가 센다. 이 모듈은 그 반대편,
//! 메인 스레드가 한 회차에 명령을 꺼내다가 **왜 멈췄는가**를 센다. 회차는 셋 중 하나로 끝난다 —
//! 큐가 비었다, 명령 수 예산에 닿았다, 시간 예산에 닿았다. 뒤의 둘은 "큐를 다 비우지 못했을 수
//! 있다" 는 신호다. **"못 비웠다" 는 아니다** — 예산에 닿은 순간 큐를 더 들여다보지 않으므로 큐가
//! 마침 비어 있었는지는 모른다. 남은 것이 있었는지는 입장 장부의 `queued_commands` 가 답한다.
//!
//! 원자값 셋이고 호출 수와 무관하게 자라지 않는다. 저장소를 거치지 않는다([ADR-0305] 와 같은
//! 축이다 — caller 로 나누지 않는 프로세스 게이지).
//!
//! [ADR-0305]: ../../../docs/adr/0305-request-pressure-is-a-process-gauge-not-a-per-caller-observation.md

use std::sync::atomic::{AtomicU64, Ordering};

/// 한 회차가 명령 꺼내기를 멈춘 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundEnd {
    /// 큐가 비었다.
    Drained,
    /// 명령 수 예산에 닿았다.
    CountBudget,
    /// 시간 예산에 닿았다.
    TimeBudget,
}

/// dispatch 쪽 누계. 올리는 자리는 메인 스레드의 회차(`app::ipc_round`) 하나뿐이다.
#[derive(Debug, Default)]
pub struct DispatchStats {
    rounds: AtomicU64,
    stopped_by_count: AtomicU64,
    stopped_by_time: AtomicU64,
    expired_before_run: AtomicU64,
}

impl DispatchStats {
    /// 명령을 하나 이상 꺼낸 회차 하나를 센다. 빈 회차는 부르지 않는다 — `queue_before_gate`
    /// 의 `drains` 와 같은 모수다.
    pub fn record_round(&self, end: RoundEnd) {
        self.rounds.fetch_add(1, Ordering::Relaxed);
        match end {
            RoundEnd::Drained => {}
            RoundEnd::CountBudget => {
                self.stopped_by_count.fetch_add(1, Ordering::Relaxed);
            }
            RoundEnd::TimeBudget => {
                self.stopped_by_time.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// 기한이 큐에서 지나 **실행하지 않은** 명령 하나를 센다. 꺼낸 쪽이 만료를 본 경우와
    /// 기다리던 쪽이 먼저 물러난 경우를 함께 센다 — 둘 다 "기한이 지나 실행 안 됨" 이다.
    pub fn record_expired_before_run(&self) {
        self.expired_before_run.fetch_add(1, Ordering::Relaxed);
    }

    /// 지금 값.
    pub fn snapshot(&self) -> DispatchSnapshot {
        DispatchSnapshot {
            rounds: self.rounds.load(Ordering::Relaxed),
            rounds_stopped_by_count: self.stopped_by_count.load(Ordering::Relaxed),
            rounds_stopped_by_time: self.stopped_by_time.load(Ordering::Relaxed),
            expired_before_run: self.expired_before_run.load(Ordering::Relaxed),
        }
    }
}

/// [`DispatchStats`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DispatchSnapshot {
    /// 명령을 하나 이상 꺼낸 회차 수.
    pub rounds: u64,
    /// 그중 명령 수 예산에 닿아 멈춘 회차.
    pub rounds_stopped_by_count: u64,
    /// 그중 시간 예산에 닿아 멈춘 회차.
    pub rounds_stopped_by_time: u64,
    /// 기한이 큐에서 지나 실행하지 않은 명령 수(`-32067` 로 답한 것).
    pub expired_before_run: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_end_is_counted_once_and_every_round_is_counted() {
        let d = DispatchStats::default();
        d.record_round(RoundEnd::Drained);
        d.record_round(RoundEnd::TimeBudget);
        d.record_round(RoundEnd::TimeBudget);
        d.record_round(RoundEnd::CountBudget);
        assert_eq!(
            d.snapshot(),
            DispatchSnapshot {
                rounds: 4,
                rounds_stopped_by_count: 1,
                rounds_stopped_by_time: 2,
                expired_before_run: 0,
            }
        );
    }
}
