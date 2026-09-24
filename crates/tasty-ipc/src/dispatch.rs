//! dispatch 회차·실행 전 만료·in-flight 요청을 센다.
//! 예산에 도달해 회차를 멈췄다는 사실만으로 큐에 잔여 요청이 있다고 단정하지 않는다.
//! 대기 중인 요청은 admission 장부가, 시작된 요청은 FlightTicket이 센다.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::admission::{AdmissionSnapshot, CommandAdmission};

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
    started: AtomicU64,
    in_flight: AtomicU64,
    in_flight_max: AtomicU64,
}

impl DispatchStats {
    /// 명령을 하나 이상 꺼낸 회차를 센다. queue_before_gate.drains와 같은 대상이다.
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

    /// 실행 시작 시 in-flight를 늘리는 ticket을 반환한다. CommandLifecycle의
    /// 마지막 보유자가 놓을 때 줄어든다. 응답 대기자가 먼저 떠나도 명령이 보유하면 유지된다.
    pub(crate) fn begin_flight(self: &Arc<Self>) -> FlightTicket {
        self.started.fetch_add(1, Ordering::Relaxed);
        let now = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        self.in_flight_max.fetch_max(now, Ordering::Relaxed);
        FlightTicket {
            stats: self.clone(),
        }
    }

    /// 지금 값.
    pub fn snapshot(&self) -> DispatchSnapshot {
        DispatchSnapshot {
            rounds: self.rounds.load(Ordering::Relaxed),
            rounds_stopped_by_count: self.stopped_by_count.load(Ordering::Relaxed),
            rounds_stopped_by_time: self.stopped_by_time.load(Ordering::Relaxed),
            expired_before_run: self.expired_before_run.load(Ordering::Relaxed),
            started: self.started.load(Ordering::Relaxed),
            in_flight: self.in_flight.load(Ordering::Relaxed),
            in_flight_max: self.in_flight_max.load(Ordering::Relaxed),
        }
    }
}

/// 실행 중인 요청 하나의 몫. 버려질 때 in-flight 에서 빠진다.
#[derive(Debug)]
pub struct FlightTicket {
    stats: Arc<DispatchStats>,
}

impl Drop for FlightTicket {
    fn drop(&mut self) {
        // 이미 0이면 줄이지 않고 경고해 unsigned wraparound를 피한다.
        if let Err(v) =
            self.stats
                .in_flight
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(1))
                })
        {
            tracing::warn!("in-flight gauge did not decrement from {v}");
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
    /// 기한이 큐에서 지나 실행하지 않은 명령 수. 소켓 요청은 `-32067` 로 답한 것이고, 호스트 주입
    /// 명령도 센다 — 주입 명령은 `-32067` 로 답해지지 않고 호출자 스레드에서
    /// [`crate::host_call::InjectError::Expired`] 로 끝난다(docs/architecture/ipc-server.md#기한).
    pub expired_before_run: u64,
    /// 실행을 시작한 명령 수의 누계.
    pub started: u64,
    /// 실행을 시작한 뒤 명령 또는 응답 대기자가 lifecycle을 보유한 요청 수.
    ///
    /// 메인 스레드의 동기 handler 는 한 번에 하나라, 이 값이 1 을 넘는 것은 응답을 워커로 넘긴
    /// 요청(`approval.await` · `agent.task_await` · plugin namespace 호출 등)이 기다리는 동안이다.
    /// 명령과 응답 대기자가 공유한 lifecycle을 모두 놓을 때 빠진다. 호출자가 기다리기를
    /// 멈춰도 실행 중인 명령이 lifecycle을 보유하면 계속 센다. lifecycle 없이 계속되는
    /// 백그라운드 작업 전체를 세는 값은 아니다. 연결 수(`connections.live`)와도 구분한다.
    pub in_flight: u64,
    /// 지금까지 본 `in_flight` 의 최댓값.
    pub in_flight_max: u64,
}

/// 큐 입장 장부와 dispatch 통계를 함께 제공한다. 각 원천의 의미는 그대로 유지한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandQueueSnapshot {
    /// 큐에 든 바이트·명령 수·거절 누계. 서버가 안 뜬 조립이면 `None`.
    pub admission: Option<AdmissionSnapshot>,
    /// 회차 · 실행 전 만료 · in-flight.
    pub dispatch: DispatchSnapshot,
}

impl CommandQueueSnapshot {
    /// 두 원천을 한 시점으로 읽는다. 두 읽기 사이의 원자성은 없다 — 각자 원자값이고 진단용이다.
    pub fn read(admission: Option<&CommandAdmission>, dispatch: &DispatchStats) -> Self {
        Self {
            admission: admission.map(CommandAdmission::snapshot),
            dispatch: dispatch.snapshot(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flight_counts_while_its_ticket_lives_and_keeps_its_peak() {
        let d = Arc::new(DispatchStats::default());
        let a = d.begin_flight();
        let b = d.begin_flight();
        assert_eq!((d.snapshot().in_flight, d.snapshot().in_flight_max), (2, 2));
        drop(a);
        assert_eq!(d.snapshot().in_flight, 1);
        drop(b);
        let s = d.snapshot();
        assert_eq!((s.in_flight, s.in_flight_max, s.started), (0, 2, 2));
    }

    #[test]
    fn the_queue_snapshot_carries_the_ledger_as_is() {
        let ledger = CommandAdmission::new(crate::admission::QueueLimits::DEFAULT);
        let t = ledger
            .admit(40, crate::admission::Origin::Socket)
            .expect("admit");
        let d = DispatchStats::default();
        let s = CommandQueueSnapshot::read(Some(&ledger), &d);
        assert_eq!(s.admission, Some(ledger.snapshot()));
        assert_eq!(s.admission.map(|a| a.queued_bytes), Some(40));
        drop(t);
        assert_eq!(CommandQueueSnapshot::read(None, &d).admission, None);
    }

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
                ..DispatchSnapshot::default()
            }
        );
    }
}
