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
        // 다른 윈도우가 바꾼 공용 목록(Explorer·포트 즐겨찾기)을 그리기 전에 받는다.
        if session.runtime.sync_shared_lists() {
            tracing::debug!(window = ?id, "shared lists: copy refreshed before redraw");
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
        self.redraw_windows_for_shared_lists();
    }

    /// 공용 목록(Explorer·포트 즐겨찾기) 원본이 마지막으로 알린 리비전 뒤에 바뀌었으면 모든 메인 윈도우를
    /// 다시 그리게 한다. 변경은 프레임 안의 intent 루프에서도, 프레임 밖의 `dispatch_pending_intents` 에서도
    /// 적용되므로 두 곳 모두 이것을 부른다. 각 윈도우는 그리기 전에 사본을 맞춘다.
    pub(crate) fn redraw_windows_for_shared_lists(&mut self) {
        let registries = &self.services.registries;
        redraw_other_windows(
            [
                registries.explorer_favorites.revision(),
                registries.port_favorites.revision(),
            ],
            &mut self.shared_lists_announced,
            self.view
                .views
                .values_mut()
                .filter_map(|view| view.as_main_mut()),
            crate::view::ui::View::mark_dirty,
        );
    }
}

/// `announced` 뒤에 원본 리비전이 바뀌었으면 기준을 올리고 모든 윈도우에 `redraw` 를 부른다.
fn redraw_other_windows<K: PartialEq + Copy, W>(
    current: K,
    announced: &mut K,
    windows: impl IntoIterator<Item = W>,
    mut redraw: impl FnMut(W),
) {
    if current == *announced {
        return;
    }
    *announced = current;
    for window in windows {
        redraw(window);
    }
}

#[cfg(test)]
mod tests {
    use crate::core::explorer_favorites::ExplorerFavorites;
    use crate::core::port_favorites::PortFavorites;
    use crate::core::shared_list::SharedList;

    /// 프레임 밖에서 적용된 제거도, 다른 목록의 변경도 다음 확인 때 모든 윈도우를 한 번 깨운다.
    #[test]
    fn a_shared_list_change_redraws_every_window_once() {
        let _home = crate::test_support::IsolatedHome::new();
        let explorer = SharedList::<ExplorerFavorites>::default();
        let port = SharedList::<PortFavorites>::default();
        let current = || [explorer.revision(), port.revision()];
        let mut announced = current();
        let mut redrawn = [0, 0];
        let check = |announced: &mut [u64; 2], redrawn: &mut [i32; 2]| {
            super::redraw_other_windows(current(), announced, redrawn.iter_mut(), |n| *n += 1)
        };
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [0, 0], "nothing changed");

        let path = crate::test_support::abs_path("w/alpha");
        explorer.update(|f| f.add(path.clone(), String::new()));
        explorer.update(|f| f.remove(&path));
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1]);
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1], "already announced");

        port.update(|f| f.add("127.0.0.1".parse().unwrap(), 3000, String::new()));
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [2, 2], "a port favorite change also redraws");
    }
}
