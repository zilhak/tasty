//! Shutdown 직전 살아있는 surface 들에 대한 close cascade.
//!
//! 일반 close 경로 (사용자 탭 X / `tasty close surface` 등) 는 각 cascade 가
//! `enqueue_surface_closed` 를 호출하지만, host shutdown 은 event_loop 가 즉시
//! 빠지므로 `pending_lifecycle_events` 도, plugin 측 cleanup 기회도 없다.
//! 본 모듈은 그 gap 을 메운다.
//!
//! 단계별 계측(S1~S4, `shutdown_total`)은 `target: "tasty::shutdown"` 으로 상시
//! 발화한다 — 마커 표는 [`docs/architecture/shutdown-sequence.md`].

use std::time::Instant;

use crate::app::{App, shutdown_trace};

impl App {
    /// 모든 main view + parked state 의 살아있는 surface 에 대해
    /// `enqueue_surface_closed(sid, kind, is_user_close=true)` 호출.
    ///
    /// drain 은 별도 호출 (`dispatch_pending_surface_lifecycle`). 본 메서드는
    /// state 큐에 push 만 한다 — borrow / 순회 순서 단순화 목적.
    ///
    /// 반환: 큐에 push 한 surface 수 (S3 계측의 `surfaces` 필드).
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
        let count = targets.len();
        for (sid, kind) in targets {
            // is_user_close=true — Cmd-Q / quit modal 은 사용자 의지 종료.
            state.enqueue_surface_closed(sid, kind, true);
        }
        count
    }

    /// 종료 시퀀스 진입점 — 세 진입 경로(`AppEvent::Shutdown`, quit modal 즉시
    /// 종료, `close_behavior=="quit"`)가 **공유**한다.
    ///
    /// 진입점을 하나로 모은 이유는 두 가지다: ① layout 저장(S1)이 반드시 close
    /// cascade 앞에 와야 한다는 순서 제약을 호출자별로 재현하지 않기 위해,
    /// ② 계측 t0 이 경로마다 어긋나면 `shutdown_total` 의 의미가 흔들리기 때문.
    pub(crate) fn begin_shutdown(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        shutdown_trace::mark_start();

        // 단계 1: layout.json 저장. layout 은 *살아있는* surface 상태를 기록해야
        //         하므로 surface.closed 발화(= layout 에서 사라지는 의미) 전에
        //         저장이 끝나 있어야 한다.
        let t_flush = Instant::now();
        self.flush_layout_persistence(true);
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t_flush),
            "S1 layout_flush (SaveLayoutNow force — main + parked engine)"
        );

        self.shutdown_lifecycle_cascade(event_loop);
    }

    /// `begin_shutdown` 후반부 — plugin/surface lifecycle 정리 후 event loop 탈출.
    fn shutdown_lifecycle_cascade(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let t0 = shutdown_trace::mark_start();

        // 단계 0: 부팅 중(WaitingEngine) 종료라면 워커가 spawn 한 plugin 자식
        //         프로세스를 먼저 회수한다 (steady-state 에선 no-op).
        //         계측은 S2 — 회수가 실제로 일어나는 경로에서만 발화한다.
        self.reclaim_boot_engine_worker_for_exit();

        // 단계 1: shutdown initiated 이벤트 발화. plugin 이 cleanup hook 을 돌릴 시간을 준다.
        self.emit_shutdown_initiated();

        // 단계 2+3: surface close cascade (S3).
        self.shutdown_close_surfaces();

        // 단계 3.5: surface close 가 뒤로 미뤄둔 observer sink 워커 회수 (S3b).
        //           close 경로는 렌더 스레드를 막지 않으려고 join 을 미루므로
        //           (`ObserverRouter::drop_surface`), 프로세스가 끝나기 전에 여기서
        //           한 번 걷어야 마지막 항목까지 sink 에 남는다. 워커들은 그동안
        //           병렬로 배수를 끝냈으므로 정상 경로 실측 대기는 0 에 가깝다.
        self.shutdown_join_observer_sinks();

        // 단계 4: plugin 종료 (S4/S4a).
        self.shutdown_plugins();

        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t0),
            "shutdown_total (shutdown enter -> event_loop.exit())"
        );
        event_loop.exit();
    }

    /// 단계 1 — `system.shutdown_initiated` 발화. 계측 없음(채널 send 뿐이라
    /// 저비용이며, 실측이 이를 뒤집으면 그때 마커를 붙인다).
    fn emit_shutdown_initiated(&mut self) {
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

    /// 단계 2+3 — 살아있는 surface 들에 close cascade 큐 push 후 즉시 drain.
    ///
    /// 단계 2 는 plugin (예: claude / codex) 이 child registry 등 자기 state 를
    /// 정리할 마지막 기회이고, 단계 3 은 그 pending 이벤트를 plugin 으로
    /// broadcast 한다 — event_loop 가 곧 빠져 다음 `about_to_wait` 가 없으므로
    /// 명시 호출이 필요하다. 두 단계는 "surface 를 닫아 plugin 에 알린다" 는 한
    /// 덩어리라 S3 하나로 잰다.
    fn shutdown_close_surfaces(&mut self) {
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

    /// 단계 3.5 — close 경로가 미뤄둔 observer sink 워커를 회수한다.
    ///
    /// surface close 는 워크스페이스 close 시 leaf surface 수만큼 반복되므로 그
    /// 자리에서 join 하지 않는다(`ObserverRouter::drop_surface`). 미뤄진 join 을
    /// 프로세스 종료 전에 소화하는 곳이 여기다 — 안 걸면 아직 배수 중인 워커가
    /// 프로세스와 함께 죽어 sink 의 마지막 항목이 잘린다.
    fn shutdown_join_observer_sinks(&mut self) {
        let t = Instant::now();
        // close cascade 와 같은 범위를 돈다 — 창별 engine + parked engine 전부.
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

    /// 단계 4 — plugin 종료 (2s graceful + force kill).
    ///
    /// 앞선 단계 3 의 event.dispatch 들이 같은 `req_tx` 채널에 먼저 쌓였으므로
    /// plugin worker 는 shutdown 처리 전에 surface.closed 들을 순서대로 받는다.
    /// S4 / S4a 마커는 `PluginManager::shutdown_all` 안에서 발화한다.
    fn shutdown_plugins(&mut self) {
        if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.shutdown_all();
        } else {
            // manager 자체가 없는 경우도 남긴다 — "안 걸렸다" 와 "계측이 안 붙었다"
            // 를 로그만 보고 구분할 수 있어야 한다.
            tracing::info!(
                target: "tasty::shutdown",
                ms = 0.0,
                plugins = 0,
                "S4 plugin_shutdown (no plugin manager)"
            );
        }
    }
}
