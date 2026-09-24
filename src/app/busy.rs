//! `Tick::Busy` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Busy tick에서 상태를 갱신하고 busy·attention·cwd 변경을 점유 클라이언트에 전송한다.
    /// 전송 지연이나 실패가 있을 수 있어 원격 반영 시각은 보장하지 않는다.
    pub(crate) fn poll_busy_states(&mut self) {
        let hub = self.stream_hub.clone();
        for w in self.view.views.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => {
                    let mut changed = crate::core::Core::update_busy_surfaces(&mut main.core_state);
                    // 상태바는 포커스된 surface만 표시하므로 불필요한 Git 조회를 피한다.
                    let focused = main.state.focused_surface_id(&main.core_state);
                    changed |= main.core_state.refresh_status_bar_branch(focused);
                    main.core_state.forward_busy_activity(&hub);
                    main.core_state.forward_attention(&hub);
                    main.core_state.forward_surface_cwd(&hub);
                    close_stale_mouse_capture_banners(&mut main.state, &main.core_state);
                    changed
                }
                None => false,
            };
            if changed {
                w.mark_dirty();
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            // 창이 없는 상태에서는 상태바용 브랜치 조회와 redraw가 필요 없다.
            crate::core::Core::update_busy_surfaces(engine);
            engine.forward_busy_activity(&hub);
            engine.forward_attention(&hub);
            engine.forward_surface_cwd(&hub);
        }
    }
}

/// foreground 세대가 바뀌면 마우스 캡처 배너만 닫는다. 같은 surface의 다른 배너는 유지한다.
fn close_stale_mouse_capture_banners(
    state: &mut crate::state::AppState,
    core_state: &crate::core::CoreState,
) {
    use crate::adapters::ui::BannerScope;
    use crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE;

    let stale: Vec<BannerScope> = state
        .banners
        .shown_banners()
        .filter(|b| b.id == BANNER_MOUSE_CAPTURE)
        .filter_map(|b| {
            let BannerScope::Surface(sid) = b.scope else {
                return None;
            };
            let origin = b.origin_generation?;
            (origin != core_state.foreground_generation(sid)).then(|| b.scope.clone())
        })
        .collect();
    for scope in stale {
        state
            .banners
            .close_shown_if_id(&scope, BANNER_MOUSE_CAPTURE);
    }
}
