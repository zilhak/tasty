//! 종료 전에 surface 닫힘을 알리고 observer·플러그인 정리를 진행한다.
//! 호출 순서와 폴링 대기는 shutdown_machine이 담당한다.
//! 계측 마커: docs/architecture/shutdown-sequence.md.

use std::time::Instant;

use crate::app::{App, shutdown_trace};

impl App {
    /// 창과 parked engine의 닫힘 알림을 큐에 넣는다. 반환값은 추가한 surface 수다.
    /// 실제 이벤트 전달은 dispatch_pending_surface_lifecycle을 별도로 호출한다.
    pub(crate) fn cascade_shutdown_close_all_surfaces(&mut self) -> usize {
        let mut closed = 0;
        for session in self.engines.all_sessions_mut() {
            let targets: Vec<_> = session
                .runtime
                .surfaces
                .iter()
                .map(|(id, surface)| {
                    (
                        *id,
                        surface.kind().to_owned(),
                        surface
                            .as_any()
                            .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
                            .map(|remote| remote.handles().binding()),
                    )
                })
                .collect();
            closed += targets.len();
            for (surface_id, kind, binding) in targets {
                if let Some(binding) = binding {
                    session
                        .runtime
                        .pending_plugin_retirements
                        .push((surface_id, binding));
                }
                session.runtime.pending_lifecycle_events.push(
                    crate::core::host_event::PendingSurfaceClosed {
                        surface_id,
                        kind: Some(kind),
                        is_user_close: true,
                    },
                );
            }
        }
        closed
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

    /// Disconnect sink producers without waiting on their I/O.
    pub(super) fn shutdown_begin_observer_sinks(&mut self) {
        for session in self.engines.all_sessions_mut() {
            session.observer_router.begin_shutdown();
        }
    }

    pub(super) fn shutdown_poll_observer_sinks(
        &mut self,
    ) -> crate::output_observer::ShutdownProgress {
        let mut progress = crate::output_observer::ShutdownProgress::default();
        for session in self.engines.all_sessions_mut() {
            progress.merge(session.observer_router.poll_shutdown());
        }
        progress
    }

    pub(super) fn shutdown_fence_observer_submissions(&self) {
        for session in self.engines.all_sessions() {
            session.observer_router.fence_submissions();
        }
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
