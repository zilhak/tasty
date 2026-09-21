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
    pub(crate) fn process_ipc(&mut self) -> bool {
        // `ipc` 참조를 짧게 유지: 큐가 빌 때까지 cmd 들을 한 번에 drain.
        // try_recv 결과는 owned `IpcCommand` 이므로 borrow 가 cmd 안으로 따라 가지 않는다.
        let mut pending: Vec<crate::ipc::server::IpcCommand> = Vec::new();
        let Some(ipc) = self.hub.ipc_server.as_ref() else {
            return false;
        };
        // 회차 예산 — 큐가 회차의 길이를 정하지 못하게 한다. 남은 것은 넣는 쪽이
        // 명령마다 부른 waker 가 다시 들여보낸다(`DRAIN_BUDGET_PER_ROUND` 참조).
        for _ in 0..crate::adapters::production::tcp_ipc_server::DRAIN_BUDGET_PER_ROUND {
            match ipc.try_recv() {
                Ok(cmd) => pending.push(cmd),
                Err(_) => break,
            }
        }
        if pending.is_empty() {
            return false;
        }
        // 이 프레임이 집어 든 명령 수 — 비어 있을 때는 위에서 빠지므로 여기 세는
        // 것은 "명령이 있었던 프레임" 뿐이다. 예산에 붙은 값이 나오면 그 회차는
        // 큐를 다 비우지 못한 것이다.
        self.core.pressure().record_drain(pending.len());

        let mut processed = false;
        let mut tool_registry_dirty = false;
        for cmd in pending {
            match self.ipc_dispatch_command(cmd) {
                #[cfg(debug_assertions)]
                IpcStep::Shutdown => return true,
                IpcStep::HandledDirty => {
                    tool_registry_dirty = true;
                    processed = true;
                }
                IpcStep::Handled => processed = true,
                IpcStep::NotHandled => {}
            }
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
