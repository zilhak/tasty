//! Application frame sequencing: View geometry first, engine effects second, readonly drawing last.
use super::App;
impl App {
    pub(crate) fn redraw_main_window(&mut self, id: winit::window::WindowId) {
        self.dispatch_pending_intents();
        if self.journal.is_halted() || self.journal.pauses_observation() {
            return;
        }
        self.poll_port_scans();
        self.refresh_approval_presentations();
        let Some(engine) = self.engines.of_window(id) else {
            return;
        };
        let Some(session) = self.engines.session_mut(engine) else {
            return;
        };
        let Some(view) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_main_mut())
        else {
            return;
        };
        // 다른 윈도우가 바꾼 Explorer 즐겨찾기를 그리기 전에 받는다.
        if session.runtime.sync_explorer_favorites() {
            tracing::debug!(window = ?id, "explorer: favorites copy refreshed before redraw");
            crate::view::ui::View::mark_dirty(view);
        }
        view.prepare_redraw(&session.read());
        let pending = view.state.take_pending_intents();
        for intent in pending {
            match intent.body {
                crate::intent::Intent::Engine(action) => {
                    action.apply(&mut session.borrow_mut(), self.plugin_manager.as_ref())
                }
                _ => view.state.dispatch_intent(intent),
            }
        }
        // TerminalStore owns grid → tap → throttled OS resize. Input contexts see that cut.
        view.prepare_render_inputs(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        for intent in view.state.take_pending_intents() {
            match intent.body {
                crate::intent::Intent::Engine(action) => {
                    action.apply(&mut session.borrow_mut(), self.plugin_manager.as_ref())
                }
                _ => view.state.dispatch_intent(intent),
            }
        }
        if let Some(manager) = self.plugin_manager.as_ref() {
            for update in super::view_mesh::relay_subscribed_mesh(
                &mut session.borrow_mut(),
                manager,
                &self.stream_hub,
            ) {
                match update {
                    super::view_mesh::MeshViewUpdate::Bootstrap(boot) => {
                        view.note_mesh_bootstrap(*boot)
                    }
                    super::view_mesh::MeshViewUpdate::Full { surface } => {
                        view.request_mesh_full(surface)
                    }
                }
            }
        }
        view.render_if_dirty(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        // A banner decision from this frame must update its gate before native reload/sync.
        for intent in view.state.take_pending_intents() {
            match intent.body {
                crate::intent::Intent::Engine(action) => {
                    action.apply(&mut session.borrow_mut(), self.plugin_manager.as_ref())
                }
                _ => view.state.dispatch_intent(intent),
            }
        }
        view.finish_redraw(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        super::webview_sync::synchronize(
            view,
            &mut session.borrow_mut(),
            self.plugin_manager.as_ref(),
            &self.services.navigation_proofs,
        );
        self.process_remote_tool_requests(id);
        self.poll_port_scans();
        self.redraw_windows_for_favorites();
    }

    /// Explorer 즐겨찾기 원본이 마지막으로 알린 리비전 뒤에 바뀌었으면 모든 메인 윈도우를 다시 그리게 한다.
    /// 변경은 프레임 안의 intent 루프에서도, 프레임 밖의 `dispatch_pending_intents` 에서도 적용되므로
    /// 두 곳 모두 이것을 부른다. 각 윈도우는 그리기 전에 사본을 맞춘다.
    pub(crate) fn redraw_windows_for_favorites(&mut self) {
        redraw_other_windows(
            &self.services.registries.explorer_favorites,
            &mut self.favorites_announced,
            self.view
                .views
                .values_mut()
                .filter_map(|view| view.as_main_mut()),
            |view| crate::view::ui::View::mark_dirty(view),
        );
    }
}

/// `announced` 뒤에 원본이 바뀌었으면 기준을 올리고 모든 윈도우에 `redraw` 를 부른다.
fn redraw_other_windows<W>(
    favorites: &crate::core::explorer_favorites::SharedExplorerFavorites,
    announced: &mut u64,
    windows: impl IntoIterator<Item = W>,
    mut redraw: impl FnMut(W),
) {
    let revision = favorites.revision();
    if revision == *announced {
        return;
    }
    *announced = revision;
    for window in windows {
        redraw(window);
    }
}

#[cfg(test)]
mod tests {
    /// 프레임 밖에서 적용된 제거도 다음 확인 때 모든 윈도우를 한 번 깨운다.
    #[test]
    fn a_favorites_change_redraws_every_window_once() {
        let _home = crate::test_support::IsolatedHome::new();
        let favorites = crate::core::explorer_favorites::SharedExplorerFavorites::default();
        let mut announced = favorites.revision();
        let mut redrawn = [0, 0];
        let mut check = |announced: &mut u64, redrawn: &mut [i32; 2]| {
            super::redraw_other_windows(&favorites, announced, redrawn.iter_mut(), |n| *n += 1)
        };
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [0, 0], "nothing changed");

        let path = crate::test_support::abs_path("w/alpha");
        favorites.update(|f| f.add(path.clone(), String::new()));
        favorites.update(|f| f.remove(&path));
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1]);
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1], "already announced");
    }
}
