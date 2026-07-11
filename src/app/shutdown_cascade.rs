//! Shutdown 직전 살아있는 surface 들에 대한 close cascade.
//!
//! 일반 close 경로 (사용자 탭 X / `tasty close surface` 등) 는 각 cascade 가
//! `enqueue_surface_closed` 를 호출하지만, host shutdown 은 event_loop 가 즉시
//! 빠지므로 `pending_lifecycle_events` 도, plugin 측 cleanup 기회도 없다.
//! 본 모듈은 그 gap 을 메운다.

use crate::app::App;

impl App {
    /// 모든 main view + parked state 의 살아있는 surface 에 대해
    /// `enqueue_surface_closed(sid, kind, is_user_close=true)` 호출.
    ///
    /// drain 은 별도 호출 (`dispatch_pending_surface_lifecycle`). 본 메서드는
    /// state 큐에 push 만 한다 — borrow / 순회 순서 단순화 목적.
    pub(crate) fn cascade_shutdown_close_all_surfaces(&mut self) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                Self::enqueue_close_for_engine(&mut main.state, &main.core_state);
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            Self::enqueue_close_for_engine(state, engine);
        }
    }

    fn enqueue_close_for_engine(
        state: &mut crate::state::AppState,
        engine: &crate::core::CoreState,
    ) {
        // 모든 workspace → pane → tab → leaf surface 순회.
        // `Tab::for_each_surface` 가 layout 안의 leaf surface 만 visit 하므로
        // `cascade_surface_closed` 의 `cleanup_targets` walk 와 동일한 단위.
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
        for (sid, kind) in targets {
            // is_user_close=true — Cmd-Q / quit modal 은 사용자 의지 종료.
            state.enqueue_surface_closed(sid, kind, true);
        }
    }

    /// `AppEvent::Shutdown` + quit modal 의 quit 분기 공용 종료 시퀀스.
    ///
    /// 호출자가 사전에 `flush_layout_persistence(true)` 를 끝낸 상태여야 한다.
    /// layout.json 은 *살아있는* surface 상태를 기록해야 하므로, surface.closed
    /// 발화 (= layout 에서 사라지는 의미) 전에 저장이 끝나 있어야 한다.
    pub(crate) fn shutdown_lifecycle_cascade(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        // 단계 0: 부팅 중(WaitingEngine) 종료라면 워커가 spawn 한 plugin 자식
        //         프로세스를 먼저 회수한다 (steady-state 에선 no-op).
        self.reclaim_boot_engine_worker_for_exit();

        // 단계 1: shutdown initiated 이벤트 발화. plugin 이 cleanup hook 을 돌릴 시간을 준다.
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

        // 단계 2: 살아있는 surface 들에 대한 close cascade 큐 push.
        //         plugin (예: claude / codex) 이 child registry 등 자기 state 를
        //         정리할 마지막 기회.
        self.cascade_shutdown_close_all_surfaces();

        // 단계 3: drain — pending_lifecycle_events 를 plugin 으로 broadcast.
        //         event_loop 가 곧 빠지므로 다음 about_to_wait 가 없어 명시 호출 필요.
        self.dispatch_pending_surface_lifecycle();

        // 단계 4: plugin 종료 (2s graceful + force kill).
        //         위 3) 의 event.dispatch 들이 같은 req_tx 채널에 먼저 쌓였으므로
        //         plugin worker 가 shutdown 처리 전에 surface.closed 들을 순서대로 받음.
        if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.shutdown_all();
        }

        event_loop.exit();
    }
}
