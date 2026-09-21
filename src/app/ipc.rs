//! IPC dispatch loop — `about_to_wait` 에서 매 프레임 1번 drain.
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

impl App {
    /// Process pending IPC commands. Returns true if any commands were processed.
    ///
    /// 한 회차의 규칙(수 예산 · 시간 예산 · 이월)은 [`crate::app::ipc_round`] 가 정한다 —
    /// headless `pump_ipc` 와 같은 것을 쓴다.
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
        round.finish(self.core.pressure(), self.core.dispatch());
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
