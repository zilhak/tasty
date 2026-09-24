//! 종료 전에 surface 닫힘을 알리고 observer·플러그인 정리를 진행한다.
//! 호출 순서와 폴링 대기는 shutdown_machine이 담당한다.
//! 계측 마커: docs/architecture/shutdown-sequence.md.

use std::time::Instant;

use crate::app::{App, shutdown_trace};

impl App {
    /// 창과 parked engine의 닫힘 알림을 큐에 넣는다. 반환값은 추가한 surface 수다.
    /// 실제 이벤트 전달은 dispatch_pending_surface_lifecycle을 별도로 호출한다.
    pub(crate) fn cascade_shutdown_close_all_surfaces(&mut self) -> usize {
        let mut closed = 0usize;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                closed += Self::enqueue_close_for_engine(&mut main.state, &main.core_state);
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            closed += Self::enqueue_close_for_engine(state, engine);
        }
        closed
    }

    fn enqueue_close_for_engine(
        state: &mut crate::state::AppState,
        engine: &crate::core::CoreState,
    ) -> usize {
        let mut targets: Vec<(u32, Option<&'static str>)> = Vec::new();
        for ws in &engine.workspaces {
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        tab.for_each_surface(&mut |s| {
                            if let Some(id) = s.surface_id() {
                                targets.push((id, Some(s.kind())));
                            }
                        });
                    }
                }
            }
        }
        let count = targets.len();
        for (sid, kind) in targets {
            // 앱 종료는 사용자 닫기로 알린다.
            state.enqueue_surface_closed(sid, kind, true);
        }
        count
    }

    pub(super) fn emit_shutdown_initiated(&mut self) {
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::SystemShutdownInitiated;
            mgr.emit_host_event(
                "system.shutdown_initiated",
                &SystemShutdownInitiated {
                    reason: "user_quit".to_string(),
                },
                EventScope::System,
            );
        }
    }

    /// 정상 이벤트 처리를 다시 돌지 않으므로 종료 단계에서 닫힘 알림을 직접 전달한다.
    pub(super) fn shutdown_close_surfaces(&mut self) {
        let t_cascade = Instant::now();
        let surfaces = self.cascade_shutdown_close_all_surfaces();
        self.dispatch_pending_surface_lifecycle();
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t_cascade),
            surfaces,
            "S3 surface_close_cascade (enqueue + plugin broadcast)"
        );
    }

    /// surface마다 기다리지 않고 미뤄 둔 observer 워커 회수를 종료 전에 마친다.
    /// join_retired는 동기적으로 기다리므로 이 단계의 렌더가 지연될 수 있다.
    pub(super) fn shutdown_join_observer_sinks(&mut self) {
        let t = Instant::now();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.core_state.observer_router.join_retired();
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.observer_router.join_retired();
        }
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t),
            "S3b observer_sink_join (deferred close-path joins)"
        );
    }

    /// shutdown_step_closing_surfaces에서 surface.closed를 보낸 뒤 호출한다.
    /// 종료 요청을 보내고 자식 회수는 StoppingPlugins 단계에서 폴링한다.
    pub(super) fn begin_plugin_shutdown(&mut self) {
        if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.begin_shutdown_all();
        } else {
            // 매니저가 없는 경우도 기록해 계측 누락과 구별한다.
            tracing::info!(
                target: "tasty::shutdown",
                ms = 0.0,
                plugins = 0,
                "S4 plugin_shutdown (no plugin manager)"
            );
        }
    }
}
