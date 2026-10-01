//! Application frame sequencing: View geometry first, engine effects second, readonly drawing last.
use super::App;
impl App {
    pub(crate) fn redraw_main_window(&mut self,id:winit::window::WindowId) {
        self.dispatch_pending_intents();
        if self.journal.is_halted() || self.journal.pauses_observation() {return;}
        self.refresh_approval_presentations();
        let Some(engine)=self.engines.of_window(id) else {return;};
        let Some(session)=self.engines.session_mut(engine) else {return;};
        let Some(view)=self.view.views.get_mut(&id).and_then(|view|view.as_main_mut()) else {return;};
        view.prepare_redraw(&session.read());
        let pending=view.state.take_pending_intents();
        for intent in pending {
            match intent.body {
                crate::intent::Intent::Engine(action)=>action.apply(&mut session.borrow_mut(),self.plugin_manager.as_ref()),
                _=>view.state.dispatch_intent(intent),
            }
        }
        // TerminalStore owns grid → tap → throttled OS resize. Input contexts see that cut.
        view.prepare_render_inputs(&session.read(),self.plugin_manager.as_ref().map(super::plugin_display::PluginDisplay::new));
        for intent in view.state.take_pending_intents() {
            match intent.body {
                crate::intent::Intent::Engine(action)=>action.apply(&mut session.borrow_mut(),self.plugin_manager.as_ref()),
                _=>view.state.dispatch_intent(intent),
            }
        }
        if let Some(manager)=self.plugin_manager.as_ref() {
            for update in super::view_mesh::relay_subscribed_mesh(&mut session.borrow_mut(),manager,&self.stream_hub) {
                match update {
                    super::view_mesh::MeshViewUpdate::Bootstrap {surface,plugin,width,height,ppp,theme,focused}=>view.note_mesh_bootstrap(surface,plugin,width,height,ppp,theme,focused),
                    super::view_mesh::MeshViewUpdate::Full {surface}=>view.request_mesh_full(surface),
                }
            }
        }
        view.render_if_dirty(&session.read(),self.plugin_manager.as_ref().map(super::plugin_display::PluginDisplay::new));
        view.finish_redraw(&session.read(),self.plugin_manager.as_ref().map(super::plugin_display::PluginDisplay::new));
    }
}
