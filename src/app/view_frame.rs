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
        let favorites_seen = self.services.registries.explorer_favorites.revision();
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
        if self.services.registries.explorer_favorites.revision() != favorites_seen {
            self.redraw_other_windows(id);
        }
    }

    /// 이 윈도우에서 바뀐 공용 상태를 다른 윈도우가 다음 프레임에 받아 그리게 한다.
    fn redraw_other_windows(&mut self, changed_in: winit::window::WindowId) {
        for (window, view) in self.view.views.iter_mut() {
            if *window != changed_in
                && let Some(view) = view.as_main_mut()
            {
                crate::view::ui::View::mark_dirty(view);
            }
        }
    }
}
