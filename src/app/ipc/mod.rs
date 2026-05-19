//! IPC dispatch loop — `about_to_wait` 에서 매 프레임 1번 drain.
//!
//! 본 모듈은 loop 본체만 담고, 단계별 처리는 sub-module 로 분산.
//!
//! 단계 순서 (먼저 매칭되는 step 에서 종료):
//!
//! 1. `caller_gate`: session_token → CallerContext 결정 + Agent caller 의 `ensure_allowed`
//!    (audit + capability elevation 처리). Local caller 는 그대로 통과.
//! 2. `app_methods`: 호스트 자체 메서드 (script.reload / system.shutdown /
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
    /// `system.shutdown` 만 — loop 즉시 종료, true 반환.
    Shutdown,
}

impl App {
    /// Process pending IPC commands. Returns true if any commands were processed.
    pub(crate) fn process_ipc(&mut self) -> bool {
        // `ipc` 참조를 짧게 유지: 큐가 빌 때까지 cmd 들을 한 번에 drain.
        // try_recv 결과는 owned `IpcCommand` 이므로 borrow 가 cmd 안으로 따라 가지 않는다.
        let mut pending: Vec<crate::ipc::server::IpcCommand> = Vec::new();
        let Some(ipc) = self.engine.ipc_server.as_ref() else {
            return false;
        };
        while let Ok(cmd) = ipc.try_recv() {
            pending.push(cmd);
        }
        if pending.is_empty() {
            return false;
        }

        let mut processed = false;
        let mut tool_registry_dirty = false;
        for cmd in pending {
            let caller = match self.ipc_resolve_caller(&cmd) {
                Some(c) => c,
                None => {
                    processed = true;
                    continue;
                }
            };
            match self.ipc_step_app_methods(&cmd, &caller) {
                IpcStep::Shutdown => return true,
                IpcStep::HandledDirty => {
                    tool_registry_dirty = true;
                    processed = true;
                    continue;
                }
                IpcStep::Handled => {
                    processed = true;
                    continue;
                }
                IpcStep::NotHandled => {}
            }
            #[cfg(debug_assertions)]
            if matches!(self.ipc_step_debug(&cmd), IpcStep::Handled) {
                processed = true;
                continue;
            }
            if matches!(self.ipc_step_window_required(&cmd), IpcStep::Handled) {
                processed = true;
                continue;
            }
            if matches!(self.ipc_step_routing(&cmd, &caller), IpcStep::Handled) {
                processed = true;
                continue;
            }
        }
        if tool_registry_dirty {
            self.refresh_tool_registry();
        }
        processed
    }
}
