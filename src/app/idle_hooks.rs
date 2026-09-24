//! Busy tick에서 surface별 IdleTimeout 훅을 확인한다.
//! engine이 반환한 일치 결과의 바인딩 실행·호스트 이벤트 전달은 App이 담당한다.
//! engine 단위 GlobalHookManager와는 별개다.

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
