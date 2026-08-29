//! `Tick::Busy` 처리 — 모든 surface 의 busy 상태 갱신.

use crate::app::App;

impl App {
    /// Refresh the busy-surface cache for every live AppState. Triggered ~1s
    /// from the central timer hub via `Tick::Busy`. Marks any window
    /// whose set actually changed as dirty so the indicators redraw.
    ///
    /// Also forwards busy transitions to any attach client occupying one of
    /// this instance's surfaces (`StreamControl::Activity`) — the same 1Hz tick
    /// that refreshes the local busy cache doubles as the cadence for that
    /// push, so a remote mirror's status dot never lags local by more than one
    /// tick. `stream_hub` is cloned once up front (cheap — internal `Arc`) so
    /// the per-engine forward calls don't need to borrow all of `self` while
    /// `self.view.views.values_mut()` already holds a mutable borrow.
    pub(crate) fn poll_busy_states(&mut self) {
        let hub = self.stream_hub.clone();
        for w in self.view.views.values_mut() {
            let changed = match w.as_main_mut() {
                Some(main) => {
                    let mut changed = crate::core::Core::update_busy_surfaces(&mut main.core_state);
                    // StatusBar 브랜치 캐시도 같은 1Hz tick 에 편승한다. 갱신 대상은
                    // **focus surface 하나뿐** — 전 surface 순회는 surface 수만큼 초당
                    // IO 를 만드는데 StatusBar 는 focus 하나만 표시한다. focus 는
                    // `AppState` 소유라 `CoreState` 안에서는 알 수 없어 여기서 넘긴다
                    // (`core/state/branch.rs`).
                    let focused = main.state.focused_surface_id(&main.core_state);
                    // 브랜치 변화도 dirty 조건에 합류시킨다 — 빠뜨리면 `git checkout`
                    // 후 다음 리페인트 유발 이벤트가 올 때까지 옛 브랜치가 남는다.
                    changed |= main.core_state.refresh_status_bar_branch(focused);
                    main.core_state.forward_busy_activity(&hub);
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
            // parked 는 window 가 없어 redraw 의미가 없다. 반환값은 무시. StatusBar 도
            // 그려지지 않으므로 브랜치 캐시는 갱신하지 않는다(읽는 쪽이 없다).
            crate::core::Core::update_busy_surfaces(engine);
            engine.forward_busy_activity(&hub);
        }
    }
}

/// `BANNER_MOUSE_CAPTURE`(TUI 마우스 캡쳐 안내 배너)를 유발한 foreground 인스턴스가
/// 더 이상 현재 foreground 와 같지 않으면(그 TUI 가 종료돼 쉘로 돌아왔든, 쉘을 거치지
/// 않고 곧바로 다른 TUI 로 넘어갔든) 자동으로 닫는다.
///
/// 조건은 `origin_generation != 현재 generation` 하나로 일반화되어 있다 — "foreground
/// 가 쉘로 복귀"(가장 흔한 케이스)도 이름이 바뀌는 순간이라 이 조건에 포함되고, 쉘을
/// 거치지 않는 TUI→TUI 전이도 동일하게 잡힌다. `close_shown_if_id` 를 쓰므로 같은
/// surface 에 다른 배너(예: 셸 통합 미설치 안내)가 떠 있어도 그건 건드리지 않는다.
/// mirror(원격 attach) surface 는 로컬 foreground 폴링이 없어
/// `foreground_generation` 이 항상 0 으로 남고, mirror 에는 애초에 이 배너가 push 될
/// 경로가 없으므로(로컬 마우스 캡처 이벤트가 없음) 자연히 대상에서 제외된다.
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
