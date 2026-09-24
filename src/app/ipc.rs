//! about_to_wait와 IpcReady에서 예산 안의 IPC 회차를 처리한다.
//! 호출자 확인, App 메서드, debug·창 필요 메서드, 일반 라우팅 순서로 시도한다.

mod app_methods;
mod caller_gate;
#[cfg(debug_assertions)]
mod debug_methods;
mod routing;
mod window_required;

use crate::app::App;

pub(crate) enum IpcStep {
    NotHandled,
    Handled,
    HandledDirty,
    /// debug 종료 요청이면 현재 IPC 루프도 끝낸다.
    #[cfg(debug_assertions)]
    Shutdown,
}

/// IpcReady가 연속으로 와도 타이머·입력·렌더 처리가 실행될 수 있도록 회차 사이를 띄운다.
/// 예산에서 잘린 회차의 재깨움은 about_to_wait 사이에 한 번만 이 간격을 건너뛴다.
/// about_to_wait 없이 이벤트만 처리하는 플랫폼 모달 루프에서도 IPC를 처리할 수 있도록
/// 이벤트 경로를 유지한다. 해당 플랫폼별 동작을 모두 실측했다는 뜻은 아니다.
pub(crate) struct IpcPacer {
    last_round_end: Option<std::time::Instant>,
    /// 예산에서 잘린 회차가 남긴 재깨움을 아직 사용하지 않았다.
    rewake_owed: bool,
    /// 직전 about_to_wait 이후 간격 예외를 한 번 사용했다.
    rewake_spent: bool,
    wake: Box<dyn Fn()>,
}

impl IpcPacer {
    pub(crate) fn new(wake: Box<dyn Fn()>) -> Self {
        Self {
            last_round_end: None,
            rewake_owed: false,
            rewake_spent: false,
            wake,
        }
    }

    pub(crate) fn event_may_run_round(&mut self, now: std::time::Instant) -> bool {
        if self.rewake_owed && !self.rewake_spent {
            self.rewake_owed = false;
            self.rewake_spent = true;
            return true;
        }
        self.last_round_end.is_none_or(|end| {
            now.saturating_duration_since(end) >= crate::app::ipc_round::ROUND_TIME_BUDGET
        })
    }

    pub(crate) fn loop_reached_about_to_wait(&mut self) {
        self.rewake_spent = false;
    }

    /// 남은 명령의 wake를 이미 소비했을 수 있어 예산에서 잘렸으면 다시 깨운다.
    fn round_ended(&mut self, at: std::time::Instant, end: tasty_ipc::dispatch::RoundEnd) {
        self.last_round_end = Some(at);
        self.rewake_owed = end != tasty_ipc::dispatch::RoundEnd::Drained;
        if self.rewake_owed {
            (self.wake)();
        }
    }
}

impl App {
    /// 공용 ipc_round의 수·시간 예산으로 처리한다. 한 명령 이상 처리했으면 true다.
    pub(crate) fn process_ipc(&mut self) -> bool {
        let mut round = crate::app::ipc_round::IpcRound::begin();
        let mut processed = false;
        let mut tool_registry_dirty = false;
        while let Some(cmd) = round.next(self.hub.ipc_server.as_deref()) {
            let observed =
                crate::app::ipc_round::CommandObservation::begin(self.core.pressure(), &cmd);
            let step = self.ipc_dispatch_command(cmd);
            observed.finish(self.core.slow_requests());
            match step {
                #[cfg(debug_assertions)]
                IpcStep::Shutdown => {
                    let end = round.finish(self.core.pressure(), self.core.dispatch());
                    self.ipc_pacer.round_ended(std::time::Instant::now(), end);
                    return true;
                }
                IpcStep::HandledDirty => {
                    tool_registry_dirty = true;
                    processed = true;
                }
                IpcStep::Handled => processed = true,
                IpcStep::NotHandled => {}
            }
        }
        let end = round.finish(self.core.pressure(), self.core.dispatch());
        self.ipc_pacer.round_ended(std::time::Instant::now(), end);
        if tool_registry_dirty {
            self.refresh_tool_registry();
            self.refresh_palette_plugin_commands();
        }
        processed
    }

    /// 명령을 각 단계에 전달하고 처리·재집계·종료 여부를 반환한다. 일부 응답은 비동기로 이어진다.
    fn ipc_dispatch_command(&mut self, cmd: crate::ipc::server::IpcCommand) -> IpcStep {
        // 큐 대기와 handler 시간을 따로 기록한다. 대기 중 기한이 지났으면 게이트 전에 거절한다.
        if !crate::app::ipc_round::claim_or_answer(&cmd, self.core.dispatch()) {
            return IpcStep::Handled;
        }
        let caller = match self.ipc_resolve_caller(&cmd) {
            Some(c) => c,
            None => return IpcStep::Handled,
        };
        let checked = match self.gates_before_routing(&cmd.request, &caller) {
            Ok(checked) => checked,
            Err(response) => {
                crate::ipc::server::send_response(&cmd.response_tx, response);
                return IpcStep::Handled;
            }
        };
        match self.ipc_step_app_methods(&cmd, &caller) {
            #[cfg(debug_assertions)]
            IpcStep::Shutdown => return IpcStep::Shutdown,
            IpcStep::HandledDirty => return IpcStep::HandledDirty,
            IpcStep::Handled => return IpcStep::Handled,
            IpcStep::NotHandled => {}
        }
        if matches!(self.ipc_step_debug_layers(&cmd, &caller), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        if matches!(self.ipc_step_routing(&cmd, &checked), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }

    /// debug와 창 필요 단계를 같은 멱등 키 처리로 묶어 보존소를 한 번만 확인한다.
    #[cfg(debug_assertions)]
    fn ipc_step_debug_layers(
        &mut self,
        cmd: &crate::ipc::server::IpcCommand,
        caller: &crate::ipc::caller::CallerContext,
    ) -> IpcStep {
        if let Some(step) = crate::ipc::handler::idempotency::run_app_layer(
            caller,
            cmd,
            IpcStep::Handled,
            |step| matches!(step, IpcStep::Handled),
            |relayed| self.ipc_step_debug_layers(relayed, caller),
        ) {
            return step;
        }
        if matches!(self.ipc_step_debug(cmd), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        self.ipc_step_window_required(cmd)
    }

    #[cfg(not(debug_assertions))]
    fn ipc_step_debug_layers(
        &mut self,
        cmd: &crate::ipc::server::IpcCommand,
        _caller: &crate::ipc::caller::CallerContext,
    ) -> IpcStep {
        self.ipc_step_window_required(cmd)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use tasty_ipc::dispatch::RoundEnd;

    use super::IpcPacer;
    use crate::app::ipc_round::ROUND_TIME_BUDGET;

    fn counting() -> (IpcPacer, Rc<Cell<u32>>) {
        let wakes = Rc::new(Cell::new(0));
        let seen = Rc::clone(&wakes);
        (
            IpcPacer::new(Box::new(move || seen.set(seen.get() + 1))),
            wakes,
        )
    }

    #[test]
    fn the_first_wake_may_run_a_round() {
        assert!(counting().0.event_may_run_round(Instant::now()));
    }

    #[test]
    fn a_wake_right_after_a_round_yields_to_the_rest_of_the_loop() {
        let (mut pacer, _) = counting();
        let end = Instant::now();
        pacer.round_ended(end, RoundEnd::Drained);
        assert!(!pacer.event_may_run_round(end));
        assert!(!pacer.event_may_run_round(end + ROUND_TIME_BUDGET - Duration::from_millis(1)));
    }

    #[test]
    fn a_wake_a_budget_after_the_last_round_runs_one() {
        let (mut pacer, _) = counting();
        let end = Instant::now();
        pacer.round_ended(end, RoundEnd::Drained);
        assert!(pacer.event_may_run_round(end + ROUND_TIME_BUDGET));
    }

    #[test]
    fn a_cut_round_wakes_the_loop_once_and_a_drained_round_does_not() {
        let (mut pacer, wakes) = counting();
        let now = Instant::now();
        pacer.round_ended(now, RoundEnd::Drained);
        assert_eq!(
            wakes.get(),
            0,
            "a drained round has nothing left to carry over"
        );
        pacer.round_ended(now, RoundEnd::TimeBudget);
        assert_eq!(wakes.get(), 1);
        pacer.round_ended(now, RoundEnd::CountBudget);
        assert_eq!(wakes.get(), 2, "one wake per cut round");
    }

    #[test]
    fn the_wake_after_a_cut_round_opens_a_round_inside_the_budget() {
        let (mut pacer, _) = counting();
        let end = Instant::now();
        pacer.round_ended(end, RoundEnd::TimeBudget);
        assert!(pacer.event_may_run_round(end));
        assert!(
            !pacer.event_may_run_round(end),
            "the exemption is spent by the one round it opened"
        );
    }

    #[test]
    fn a_chain_of_cut_rounds_yields_until_the_loop_reaches_about_to_wait() {
        let (mut pacer, wakes) = counting();
        let end = Instant::now();
        pacer.round_ended(end, RoundEnd::TimeBudget);
        assert!(pacer.event_may_run_round(end));
        pacer.round_ended(end, RoundEnd::TimeBudget);
        assert_eq!(wakes.get(), 2, "the second cut round still wakes the loop");
        assert!(!pacer.event_may_run_round(end), "but its wake yields");
        pacer.loop_reached_about_to_wait();
        assert!(
            pacer.event_may_run_round(end),
            "after about_to_wait it may skip again"
        );
    }
}
