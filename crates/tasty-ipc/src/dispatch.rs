//! IPC 명령을 **큐에서 꺼낸 쪽**의 누계 — dispatch 회차가 어디서 멈췄는가, 기한이 지나 실행하지
//! 않은 명령이 몇인가, 그리고 지금 **실행 중인(in-flight)** 요청이 몇인가.
//!
//! 큐에 든 쪽은 입장 장부([`crate::admission::CommandAdmission`])가 센다. 이 모듈은 그 반대편,
//! 메인 스레드가 한 회차에 명령을 꺼내다가 **왜 멈췄는가**를 센다. 회차는 셋 중 하나로 끝난다 —
//! 큐가 비었다, 명령 수 예산에 닿았다, 시간 예산에 닿았다. 뒤의 둘은 "큐를 다 비우지 못했을 수
//! 있다" 는 신호다. **"못 비웠다" 는 아니다** — 예산에 닿은 순간 큐를 더 들여다보지 않으므로 큐가
//! 마침 비어 있었는지는 모른다. 남은 것이 있었는지는 입장 장부의 `queued_commands` 가 답한다.
//!
//! 원자값 일곱이고 호출 수와 무관하게 자라지 않는다. 저장소를 거치지 않는다([ADR-0305] 와 같은
//! 축이다 — caller 로 나누지 않는 프로세스 게이지).
//!
//! [ADR-0305]: ../../../docs/adr/0305-request-pressure-is-a-process-gauge-not-a-per-caller-observation.md

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

    /// 명령 하나가 실행을 시작했다 — in-flight 가 하나 는다. 표가 버려질 때 준다.
    ///
    /// 표는 명령의 실행 상태 칸(`crate::server::CommandLifecycle`)에 실려, 그 칸을 든 마지막
    /// 쪽이 놓을 때 버려진다 — 소켓·주입 경로에서는 **응답을 기다리던 쪽이 돌아갈 때**다.
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
        // 표 하나당 한 번만 버려지므로 0 아래로 갈 길이 없다. 그래도 포화 뺄셈으로 둔다 —
        // 게이지가 u64 최댓값으로 감기면 그 값을 읽는 쪽이 원인을 못 가른다. 클로저가 늘
        // `Some` 을 돌려주므로 `Err` 갈래는 없다.
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
    /// [`crate::host_call::InjectError::Expired`] 로 끝난다(ADR-0451).
    pub expired_before_run: u64,
    /// 실행을 시작한 명령 수의 누계.
    pub started: u64,
    /// 지금 실행 중인 요청 수 — **실행을 시작했고, 그 응답을 기다리는 쪽이 아직 기다리는** 요청.
    ///
    /// 메인 스레드의 동기 handler 는 한 번에 하나라, 이 값이 1 을 넘는 것은 응답을 워커로 넘긴
    /// 요청(`approval.await` · `agent.task_await` · plugin namespace 호출 등)이 기다리는 동안이다.
    /// 기다리던 쪽이 상한에서 돌아가면 그 요청은 여기서 빠진다 — 실행이 계속되더라도 받을 사람이
    /// 없는 일은 세지 않는다. 연결 수(`connections.live`)와 다르다: 연결은 요청 없이도 살아 있다.
    pub in_flight: u64,
    /// 지금까지 본 `in_flight` 의 최댓값.
    pub in_flight_max: u64,
}

/// 명령 큐의 한 시점 — 큐에 **든** 쪽(입장 장부)과 큐에서 **꺼낸** 쪽(dispatch 누계)을 한 번에
/// 읽는다. 진단 응답이 읽을 자리다.
///
/// 입장 장부의 값을 여기서 다시 정의하지 않는다 — 바이트의 뜻과 반납 시점은
/// [`crate::admission`] 이 정본이고, 이 타입은 그 스냅샷을 그대로 싣는다.
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
