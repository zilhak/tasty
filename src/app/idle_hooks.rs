//! `AppEvent::BusyPoll` 에 편승한 `IdleTimeout` 훅(`tasty set hook --event
//! idle-timeout:SECS`) 폴링/발화(TODO30).
//!
//! `CoreState::poll_idle_timeout_hooks` 는 순수 engine 레이어라 발사된 훅의
//! `(surface_id, FiredHook)` 만 돌려준다 — 바인딩 실행(`HostIpcInjector` 필요)과
//! host event enqueue(`AppState` 필요)는 이 App 레이어가 담당한다
//! (`app/dispatch_domain.rs` 의 bell/notification cascade 와 동일한 책임 분리).
//! `global_hooks.rs`(`GlobalHookManager`, non-surface-bound)와는 별개 훅
//! 시스템이다 — 이쪽은 surface 스코프 `tasty-hooks::HookManager`.

use crate::app::App;

impl App {
    pub(crate) fn poll_idle_timeout_hooks(&mut self) {
        let injector = self.core.host_ipc_injector.get().cloned();

        for w in self.view.views.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let fired = main.core_state.poll_idle_timeout_hooks();
            if fired.is_empty() {
                continue;
            }
            for (surface_id, f) in fired {
                crate::hook_handler::trigger::execute_binding(
                    &f.binding,
                    injector.as_ref(),
                    &f.event,
                    &f.received,
                    surface_id,
                );
                main.state
                    .enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                        hook_id: f.hook_id,
                        event_kind: "idle-timeout".to_string(),
                        surface_id,
                        exit_code: None,
                    });
            }
            main.base.dirty = true;
        }

        for (state, engine) in self.parked_states.iter_mut() {
            let fired = engine.poll_idle_timeout_hooks();
            for (surface_id, f) in fired {
                crate::hook_handler::trigger::execute_binding(
                    &f.binding,
                    injector.as_ref(),
                    &f.event,
                    &f.received,
                    surface_id,
                );
                state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                    hook_id: f.hook_id,
                    event_kind: "idle-timeout".to_string(),
                    surface_id,
                    exit_code: None,
                });
            }
        }
    }
}
