//! IPC dispatch loop — `about_to_wait` 에서 매 프레임 한 회차, `IpcReady` 에서는 양보 규칙
//! ([`IpcPacer`])이 허락할 때만 한 회차.
//!
//! 본 모듈은 loop 본체만 담고, 단계별 처리는 sub-module 로 분산.
//!
//! 단계 순서 (먼저 매칭되는 step 에서 종료):
//!
//! 1. `caller_gate`: session_token → CallerContext 결정 + Agent caller 의 `ensure_allowed`
//!    (audit + capability elevation 처리). Local caller 는 그대로 통과.
//! 2. `app_methods`: 호스트 자체 메서드 (system.shutdown /
//!    window.{create,close,focus,list} / plugin.* / approval.await).
//! 3. `debug_methods` (debug 빌드): debug.event_bus.* / debug.extension.invoke_hook /
//!    debug.popup.*.
//! 4. `window_required`: focused window 가 있어야 처리 가능 (surface.ime_*,
//!    debug.info, ui.screenshot).
//! 5. `routing`: plugin namespace forward → focused window / parked state fallback.

mod app_methods;
mod caller_gate;
#[cfg(debug_assertions)]
mod debug_methods;
mod routing;
mod window_required;

use crate::app::App;

/// 한 step 의 결과. 호출자(loop) 가 다음 동작을 결정한다.
pub(crate) enum IpcStep {
    /// 이 step 에서 처리되지 않음. 다음 step 시도.
    NotHandled,
    /// 처리됨. 다음 cmd 로.
    Handled,
    /// 처리됨 + tool registry 재집계 표시.
    HandledDirty,
    /// `system.shutdown` 만 — loop 즉시 종료, true 반환. debug 빌드 전용.
    #[cfg(debug_assertions)]
    Shutdown,
}

/// gui 에서 IPC 회차가 루프의 나머지(입력 · 타이머 · 렌더)에 차례를 넘기게 하는 규칙.
///
/// 회차는 두 자리에서 돈다 — `about_to_wait`(매 iteration 한 번)와 `IpcReady` 사용자 이벤트.
/// 뒤쪽이 그대로면 지속 부하에서 루프가 `about_to_wait` 에 영영 못 간다. winit 은 한 iteration
/// 안에서 사용자 이벤트를 **큐가 빌 때까지** 꺼내 처리하는데(x11 실측), 생산자는 명령마다 wake 를
/// 한 번 부르고 회차가 답을 주면 곧바로 다음 명령과 wake 를 보낸다. 회차가 도는 동안 wake 가
/// 다시 쌓이므로 그 큐가 비는 순간이 없고, 그동안 타이머도 창 입력도 렌더도 못 돈다.
///
/// 그래서 **사용자 이벤트가 부른 회차는 직전 회차가 끝난 뒤 한 회차 예산이 지났을 때만 돈다.**
/// 그 사이에 온 wake 는 루프를 깨우는 일만 하고, 명령은 곧 올 `about_to_wait` 의 회차가 집는다.
/// 회차가 안 돌면 답이 안 나가 새 명령도 안 오므로 사용자 이벤트 큐는 곧 빈다.
///
/// 사용자 이벤트 쪽 회차를 아예 없애지 않는 이유 — 플랫폼 모달 루프(창 크기 조절 · 메뉴 추적
/// 등)가 `about_to_wait` 없이 사용자 이벤트만 전하는 구간이 있으면, 그 구간에서 IPC 를 살리는
/// 것이 이 경로다. 그 구간이 실제로 있는지는 이 머신(x11)에서 잴 수 없다(ADR-0410).
#[derive(Default)]
pub(crate) struct IpcPacer {
    last_round_end: Option<std::time::Instant>,
}

impl IpcPacer {
    /// 사용자 이벤트가 지금 회차를 돌려도 되는가.
    pub(crate) fn event_may_run_round(&self, now: std::time::Instant) -> bool {
        self.last_round_end.is_none_or(|end| {
            now.saturating_duration_since(end) >= crate::app::ipc_round::ROUND_TIME_BUDGET
        })
    }

    fn round_ended(&mut self, at: std::time::Instant) {
        self.last_round_end = Some(at);
    }
}

impl App {
    /// Process pending IPC commands. Returns true if any commands were processed.
    ///
    /// 한 회차의 규칙(수 예산 · 시간 예산 · 이월)은 [`crate::app::ipc_round`] 가 정한다 —
    /// headless `pump_ipc` 와 같은 것을 쓴다. 회차가 예산에서 멈췄으면 루프를 한 번 더 깨운다 —
    /// 남은 명령의 wake 는 이미 소비됐을 수 있어서, 안 깨우면 다른 입력이 올 때까지 남는다.
    pub(crate) fn process_ipc(&mut self) -> bool {
        let mut round = crate::app::ipc_round::IpcRound::begin();
        let mut processed = false;
        let mut tool_registry_dirty = false;
        // `ipc_server` 빌림은 `next` 호출 안에서 끝난다 — 꺼낸 명령은 owned 라 handler 의
        // 가변 빌림과 안 겹친다.
        while let Some(cmd) = round.next(self.hub.ipc_server.as_deref()) {
            match self.ipc_dispatch_command(cmd) {
                #[cfg(debug_assertions)]
                IpcStep::Shutdown => {
                    round.finish(self.core.pressure(), self.core.dispatch());
                    self.ipc_pacer.round_ended(std::time::Instant::now());
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
        self.ipc_pacer.round_ended(std::time::Instant::now());
        if end != tasty_ipc::dispatch::RoundEnd::Drained {
            crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::IpcReady);
        }
        if tool_registry_dirty {
            self.refresh_tool_registry();
            self.refresh_palette_plugin_commands();
        }
        processed
    }

    /// 명령 하나를 단계 순서대로 끝까지 다룬다. 답은 이 안에서 나간다.
    ///
    /// 돌려주는 값은 회차가 알아야 할 것뿐이다 — 처리됐는가, tool registry 를 다시 모아야
    /// 하는가, (debug) 종료하라는 명령이었는가. 아무 단계도 답하지 않았으면 `NotHandled`.
    fn ipc_dispatch_command(&mut self, cmd: crate::ipc::server::IpcCommand) -> IpcStep {
        // 큐 체류 시간. handler 실행 시간과 **따로** 잰다 — 합쳐 두면 느린 응답을
        // 보고도 적체인지 handler 비용인지 고를 수 없다.
        self.core.pressure().record_queue_wait(cmd.queue_wait());
        // 기한이 큐에서 지났으면 실행하지 않고 답한다 — 게이트보다 앞이다(ADR-0411).
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
        #[cfg(debug_assertions)]
        if matches!(self.ipc_step_debug(&cmd), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        if matches!(self.ipc_step_window_required(&cmd), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        if matches!(self.ipc_step_routing(&cmd, &checked), IpcStep::Handled) {
            return IpcStep::Handled;
        }
        IpcStep::NotHandled
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::IpcPacer;
    use crate::app::ipc_round::ROUND_TIME_BUDGET;

    /// 회차가 한 번도 안 돌았으면 사용자 이벤트가 회차를 연다.
    #[test]
    fn the_first_wake_may_run_a_round() {
        assert!(IpcPacer::default().event_may_run_round(Instant::now()));
    }

    /// 직전 회차가 방금 끝났으면 사용자 이벤트는 회차를 안 연다 — 이것이 빠지면 지속 부하에서
    /// winit 의 사용자 이벤트 큐가 비지 않아 `about_to_wait` 가 영영 안 온다.
    #[test]
    fn a_wake_right_after_a_round_yields_to_the_rest_of_the_loop() {
        let mut pacer = IpcPacer::default();
        let end = Instant::now();
        pacer.round_ended(end);
        assert!(!pacer.event_may_run_round(end));
        assert!(!pacer.event_may_run_round(end + ROUND_TIME_BUDGET - Duration::from_millis(1)));
    }

    /// 한 회차 예산이 지나면 다시 연다 — `about_to_wait` 없이 사용자 이벤트만 오는 구간에서도
    /// IPC 가 진척한다.
    #[test]
    fn a_wake_a_budget_after_the_last_round_runs_one() {
        let mut pacer = IpcPacer::default();
        let end = Instant::now();
        pacer.round_ended(end);
        assert!(pacer.event_may_run_round(end + ROUND_TIME_BUDGET));
    }
}
