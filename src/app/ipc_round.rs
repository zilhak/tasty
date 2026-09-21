//! IPC dispatch 한 회차의 규칙 — gui `App::process_ipc` 와 headless `pump_ipc` 가 같은 것을 쓴다.
//!
//! 회차는 큐에서 명령을 **하나 꺼내 끝까지 처리하고, 다음 것을 꺼낸다.** 먼저 전부 모아 두고
//! 나중에 처리하지 않는다. 그래서 회차를 멈추면 남은 명령은 **큐에 그대로 있다** — 입장 장부의
//! 바이트·깊이에도 그대로 남고, 큐 대기 시간에도 계속 쌓인다. 모은 뒤에 멈췄다면 그 명령들은
//! 장부에서는 빠졌는데 실행은 안 된, 어디에도 안 보이는 상태가 됐을 것이다.
//!
//! 회차는 두 예산 중 먼저 닿는 것에서 멈춘다.
//!
//! - **명령 수** — [`DRAIN_BUDGET_PER_ROUND`]. 동시 연결 상한에서 파생된 값이다(ADR-0313).
//! - **경과 시간** — [`ROUND_TIME_BUDGET`]. 명령 하나를 끝낼 때마다 본다. **첫 명령은 시간과
//!   무관하게 늘 처리한다** — 그래야 회차마다 적어도 하나가 진척되고, 예산보다 비싼 명령
//!   하나가 영영 못 도는 일이 없다.
//!
//! 수 예산만으로는 회차의 **시간**이 안 잘린다. 수 예산은 연결 상한과 같은 256 이고, 명령
//! 하나의 비용은 수십 µs 에서 수십 ms 까지 벌어진다(실측과 판단의 근거는
//! [ADR-0410](../../docs/adr/0410-a-dispatch-round-also-stops-at-a-time-budget-and-callers-are-served-in-arrival-order.md)).
//!
//! **남은 것은 별도 배선 없이 다음 회차가 집는다** — ADR-0313 과 같은 근거다. 생산자는 명령마다
//! waker 를 정확히 한 번 부르고, 회차는 적어도 하나를 처리하므로 남은 명령 수보다 남은 wake
//! 수가 늘 많거나 같다. 빈 큐를 만난 회차는 곧바로 끝난다(busy-spin 없음).
//!
//! **이 예산은 이미 실행 중인 명령을 끊지 못한다.** 예산은 명령과 명령 **사이**에서만 본다.
//! 메인 스레드에서 도는 동기 handler 하나가 오래 걸리면 그 시간 동안 회차도, 타이머도, 화면도
//! 그 뒤에서 기다린다 — 선점할 수단이 없다(ADR-0313 의 "진행 보장의 한계").
//!
//! 종료 drain(`shutdown_machine`)은 이 규칙을 쓰지 않는다 — 남은 것을 전부 거절로 답해야 하는
//! 절차라 예산을 두면 답 없이 끝날 수 있다(ADR-0313).

use std::sync::Arc;
use std::time::{Duration, Instant};

use tasty_ipc::dispatch::{DispatchStats, RoundEnd};
use tasty_telemetry::PressureStats;

use crate::adapters::production::tcp_ipc_server::DRAIN_BUDGET_PER_ROUND;
use crate::ipc::server::{Claim, IpcCommand, expired_before_run_response};
use crate::ports::ipc_server::IpcServerPort;

/// 한 회차가 명령을 꺼내는 데 쓸 수 있는 시간. **파생이 아니다** — 근거는 ADR-0410.
///
/// 60 Hz 한 프레임이다. gui 는 회차가 끝나야 그 iteration 의 렌더·타이머로 넘어가므로, 이 값이
/// 곧 "IPC 부하가 화면 한 프레임보다 오래 루프를 쥐지 않는다" 는 약속이다. 명령 하나가 이보다
/// 비싸면 그 명령만큼은 넘친다(첫 명령은 늘 처리한다 — 모듈 doc).
pub(crate) const ROUND_TIME_BUDGET: Duration = Duration::from_millis(16);

/// 진행 중인 회차 하나.
pub(crate) struct IpcRound {
    started: Instant,
    taken: usize,
    count_budget: usize,
    time_budget: Duration,
    end: RoundEnd,
}

impl IpcRound {
    /// 제품 예산으로 지금 회차를 시작한다.
    pub(crate) fn begin() -> Self {
        Self::with_budgets(DRAIN_BUDGET_PER_ROUND, ROUND_TIME_BUDGET)
    }

    /// 예산을 골라 시작한다 — 시험이 작은 값을 넣는 자리.
    pub(crate) fn with_budgets(count_budget: usize, time_budget: Duration) -> Self {
        Self {
            started: Instant::now(),
            taken: 0,
            count_budget,
            time_budget,
            end: RoundEnd::Drained,
        }
    }

    /// 다음 명령. 예산에 닿았거나 큐가 비었으면(또는 서버가 없으면) `None` 이고, 그 뒤로도
    /// 계속 `None` 이다.
    pub(crate) fn next(&mut self, server: Option<&dyn IpcServerPort>) -> Option<IpcCommand> {
        if self.end != RoundEnd::Drained {
            return None;
        }
        if self.taken >= self.count_budget {
            self.end = RoundEnd::CountBudget;
            return None;
        }
        if self.taken > 0 && self.started.elapsed() >= self.time_budget {
            self.end = RoundEnd::TimeBudget;
            return None;
        }
        let cmd = server?.try_recv().ok()?;
        self.taken += 1;
        Some(cmd)
    }

    /// 회차를 닫고 센다. 하나도 못 꺼낸 회차는 세지 않는다 — 비어 있던 회차를 세면 "명령이
    /// 있었던 회차" 의 깊이 분포가 0 으로 희석된다.
    pub(crate) fn finish(self, pressure: &PressureStats, dispatch: &DispatchStats) {
        if self.taken == 0 {
            return;
        }
        // 이 회차가 처리한 명령 수. 예산에 붙은 값이 나오면 그 회차는 큐를 다 비우지 못했을
        // 수 있다.
        pressure.record_drain(self.taken);
        dispatch.record_round(self.end);
    }
}

/// 꺼낸 명령을 **실행하기 직전**에 부른다. 실행해도 되면 `true`.
///
/// 호출자가 실은 응답 대기 상한이 큐에서 기다리는 동안 지났으면 실행하지 않고 `-32067` 로
/// 답한다(ADR-0411). 기다리던 쪽이 먼저 물러났으면 그쪽이 이미 답했으므로 조용히 버린다. 두
/// 경우 모두 게이트(권한·audit·rate limit)에 닿기 **전**이라 실행되지 않은 요청이 토큰을 쓰거나
/// 감사 행을 남기지 않는다. 큐 대기 계측은 이보다 먼저다 — 만료된 요청도 큐에 앉아 있었다.
pub(crate) fn claim_or_answer(cmd: &IpcCommand, dispatch: &Arc<DispatchStats>) -> bool {
    match cmd.claim(dispatch) {
        Claim::Run => true,
        Claim::Expired { waited, bound } => {
            tracing::debug!(
                "IPC {} not run: waited {:?} in the queue past the caller's {:?} bound",
                cmd.request.method,
                waited,
                bound
            );
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            crate::ipc::server::send_response(
                &cmd.response_tx,
                expired_before_run_response(id, bound),
            );
            false
        }
        Claim::Withdrawn => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::adapters::test::mock_ipc_server::MockIpcServer;
    use crate::ipc::protocol::JsonRpcRequest;

    fn push(server: &MockIpcServer, n: usize) {
        let tx = server.tx_clone();
        for i in 0..n {
            let (resp_tx, _resp_rx) = mpsc::sync_channel(1);
            let req = JsonRpcRequest {
                response_timeout_ms: None,
                idempotency_key: None,
                jsonrpc: "2.0".to_string(),
                method: format!("test.m{i}"),
                id: Some(serde_json::Value::from(i as u64)),
                params: serde_json::Value::Null,
                session_token: None,
            };
            tx.send(IpcCommand::new(req, resp_tx)).expect("mock queue");
        }
    }

    fn drain(round: &mut IpcRound, server: &MockIpcServer) -> Vec<String> {
        let mut seen = Vec::new();
        while let Some(cmd) = round.next(Some(server)) {
            seen.push(cmd.request.method.clone());
        }
        seen
    }

    /// 수 예산에서 멈추면 남은 것은 큐에 **그대로** 남고, 다음 회차가 이어서 같은 순서로 집는다.
    #[test]
    fn a_round_stopped_by_count_leaves_the_rest_queued_in_order() {
        let server = MockIpcServer::new();
        push(&server, 5);
        let mut first = IpcRound::with_budgets(2, Duration::MAX);
        assert_eq!(drain(&mut first, &server), ["test.m0", "test.m1"]);
        assert_eq!(first.end, RoundEnd::CountBudget);
        let mut second = IpcRound::with_budgets(2, Duration::MAX);
        assert_eq!(drain(&mut second, &server), ["test.m2", "test.m3"]);
        let mut third = IpcRound::with_budgets(2, Duration::MAX);
        assert_eq!(drain(&mut third, &server), ["test.m4"]);
        assert_eq!(third.end, RoundEnd::Drained, "the queue ran dry first");
    }

    /// 시간 예산이 0 이어도 첫 명령은 처리한다 — 회차마다 진척이 하나는 있다.
    #[test]
    fn a_spent_time_budget_still_lets_the_first_command_through() {
        let server = MockIpcServer::new();
        push(&server, 3);
        let mut round = IpcRound::with_budgets(256, Duration::ZERO);
        assert_eq!(drain(&mut round, &server), ["test.m0"]);
        assert_eq!(round.end, RoundEnd::TimeBudget);
        let mut left = 0;
        while server.try_recv().is_ok() {
            left += 1;
        }
        assert_eq!(left, 2, "the other two are still queued");
    }

    /// 한 번 멈춘 회차는 다시 꺼내지 않는다 — 큐에 새것이 들어와도 그 회차의 몫이 아니다.
    #[test]
    fn a_stopped_round_stays_stopped() {
        let server = MockIpcServer::new();
        push(&server, 1);
        let mut round = IpcRound::with_budgets(1, Duration::MAX);
        assert_eq!(drain(&mut round, &server).len(), 1);
        push(&server, 1);
        assert!(round.next(Some(&server)).is_none());
    }

    /// 빈 회차는 세지 않고, 명령이 있던 회차는 처리한 수와 멈춘 이유를 한 번씩 센다.
    #[test]
    fn only_rounds_that_took_something_are_counted() {
        let pressure = PressureStats::default();
        let dispatch = DispatchStats::default();
        let server = MockIpcServer::new();

        let mut empty = IpcRound::with_budgets(4, Duration::MAX);
        assert!(empty.next(Some(&server)).is_none());
        empty.finish(&pressure, &dispatch);
        assert_eq!(pressure.snapshot().queue_drains, 0);
        assert_eq!(dispatch.snapshot().rounds, 0);

        push(&server, 3);
        let mut round = IpcRound::with_budgets(2, Duration::MAX);
        drain(&mut round, &server);
        assert_eq!(round.taken, 2);
        round.finish(&pressure, &dispatch);
        let p = pressure.snapshot();
        assert_eq!((p.queue_drains, p.queue_commands), (1, 2));
        let d = dispatch.snapshot();
        assert_eq!((d.rounds, d.rounds_stopped_by_count), (1, 1));
    }

    fn bounded(
        ms: u64,
    ) -> (
        IpcCommand,
        mpsc::Receiver<crate::ipc::protocol::JsonRpcResponse>,
    ) {
        let (resp_tx, resp_rx) = mpsc::sync_channel(1);
        let req = JsonRpcRequest {
            response_timeout_ms: Some(ms),
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: "workspace.create".to_string(),
            id: Some(serde_json::Value::from(5u64)),
            params: serde_json::Value::Null,
            session_token: None,
        };
        (IpcCommand::new(req, resp_tx), resp_rx)
    }

    /// 기한이 큐에서 지난 명령은 실행하지 않고, 꺼낸 쪽이 `-32067` 로 답하고 센다.
    #[test]
    fn a_command_past_its_deadline_is_answered_not_run_and_counted() {
        let dispatch = Arc::new(DispatchStats::default());
        let (cmd, rx) = bounded(1);
        std::thread::sleep(Duration::from_millis(5));
        assert!(!claim_or_answer(&cmd, &dispatch), "it must not run");
        let resp = rx.try_recv().expect("the taker answers it");
        assert_eq!(
            resp.error.expect("error").code,
            crate::ipc::protocol::ERR_EXPIRED_BEFORE_RUN
        );
        assert_eq!(dispatch.snapshot().expired_before_run, 1);
    }

    /// 기다리던 쪽이 먼저 물러난 명령은 조용히 버린다 — 답은 그쪽이 이미 썼다.
    #[test]
    fn a_withdrawn_command_is_dropped_without_a_second_answer() {
        let dispatch = Arc::new(DispatchStats::default());
        let (cmd, rx) = bounded(60_000);
        cmd.lifecycle().withdraw();
        assert!(!claim_or_answer(&cmd, &dispatch));
        assert!(rx.try_recv().is_err(), "no second answer");
        assert_eq!(dispatch.snapshot().expired_before_run, 1);
    }

    /// 기한 안의 명령은 실행한다.
    #[test]
    fn a_command_within_its_deadline_runs_and_is_in_flight_until_its_waiter_lets_go() {
        let dispatch = Arc::new(DispatchStats::default());
        let (cmd, _rx) = bounded(60_000);
        let waiter = cmd.lifecycle();
        assert!(claim_or_answer(&cmd, &dispatch));
        assert_eq!(dispatch.snapshot().expired_before_run, 0);
        assert_eq!(dispatch.snapshot().in_flight, 1);
        // 메인 스레드가 명령을 놓아도(응답을 워커로 넘긴 경우) 기다리는 쪽이 있는 동안은 실행 중이다.
        drop(cmd);
        assert_eq!(dispatch.snapshot().in_flight, 1);
        drop(waiter);
        assert_eq!(dispatch.snapshot().in_flight, 0);
    }

    /// 서버가 없는 조립에서는 아무것도 꺼내지 않는다.
    #[test]
    fn no_server_means_an_empty_round() {
        let mut round = IpcRound::begin();
        assert!(round.next(None).is_none());
        assert_eq!(round.taken, 0);
    }
}
