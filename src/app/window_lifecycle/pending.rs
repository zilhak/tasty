//! Additional-window continuation. Engine ownership stays in EngineRegistry until publication.
use super::*;
use crate::runtime::engine_session::EngineId;

pub(crate) struct PendingWindow {
    pub(crate) window: Arc<Window>,
    pub(super) gpu: GpuState,
    pub(super) engine: EngineId,
    pub(super) origin: WindowRequestOrigin,
    pub(super) db_init_error: Option<crate::db::DbInitError>,
    pub(super) invalid_theme_name: Option<String>,
    pub(crate) completion: Option<crate::app::event::IpcCompletion>,
    pub(super) plugin_deadline: Option<std::time::Instant>,
}

impl App {
    pub(crate) fn poll_journal_application(&mut self) {
        self.capture_published_input_targets();
        let projections:Vec<_>=self.engines().window_pairs().filter_map(|(window,main,engine)|self.engines.of_window(window).map(|id|(id,crate::model::StructurePresentationSnapshot::capture(&engine.workspaces(),&main.state.navigation)))).collect();
        for (id,presentation) in projections {
            if let Some(session)=self.engines.session_mut(id) {self.journal.update_completion_view(id,&session.core_state,&presentation);}
        }
        // Materialization belongs to App/Engine; View contributes only the displayed IDs.
        // Revisit after each completion so Busy/another activation never loses a visible leaf.
        let selected:Vec<_>=self.engines().window_pairs().filter_map(|(window,main,engine)| {
            let id=self.engines.of_window(window)?;
            let workspace=engine.workspace_at(main.state.active_workspace_index(engine.core))?;
            let surfaces=workspace.pane_layout().all_pane_ids().into_iter().filter_map(|id|workspace.pane_layout().find_pane(id))
                .filter_map(|pane|pane.tabs.get(main.state.navigation.tab_index(pane))).flat_map(|tab|tab.all_surface_ids()).collect::<Vec<_>>();
            Some((id,surfaces))
        }).collect();
        for (id,surfaces) in selected {
            let Some(session)=self.engines.get(id) else {continue;};
            for surface in surfaces {
                match self.journal.activate_restored_surface(session,surface) {
                    Ok(true)=>break,
                    Ok(false)=>{},
                    Err(error)=>{tracing::error!("selected surface restore failed: {error}");break;},
                }
            }
        }
        let mut sessions: Vec<_> = self.engines.all_sessions_mut().collect();
        if let Err(error) = self.journal.poll_bootstrap(&mut sessions,self.plugin_manager.as_mut()) {
            tracing::error!("journal publication halted: {error}");
        }
        if !self.journal.pauses_observation() {
            self.resolve_journal_requests();
            self.apply_attach_client_output();
        }
        if !self.journal.pauses_observation() && !self.journal.is_halted() {
            self.dispatch_pending_surface_lifecycle();
            self.dispatch_pending_host_events();
        }
        self.poll_preserved_window_closes();
        for id in self.journal.take_retired_engines() {
            if self.engines.session_mut(id).is_some_and(|session|session.runtime.has_pending_delivery()) {
                self.journal.defer_retired_engine_delivery(id);
            } else {drop(self.engines.finish_retiring(id));}
        }
    }

    pub(crate) fn poll_preserved_window_closes(&mut self) {
        if self.journal.pauses_observation() && !self.journal.is_halted() {return;}
        for (id,mut navigation,checkpoint) in self.engines.preserved_closes() {
            if self.journal.is_halted() {drop(self.engines.finish_retiring(id));continue;}
            if self.journal.has_pending_engine_effects(id) {continue;}
            let Some(session)=self.engines.session_mut(id) else {continue;};
            if !session.pending_resource_retirements.is_empty() || !session.pending_materializations.is_empty() || session.runtime.has_pending_delivery() {continue;}
            let Some(binding)=session.journal_binding.clone() else {continue;};
            if checkpoint.is_none() {
                navigation.reconcile(&session.core_state.workspaces());
                let active=navigation.workspace_id(&session.core_state.workspaces());
                let active_index=navigation.workspace_index(&session.core_state.workspaces());
                self.journal.queue_surface_capture(session,true);
                self.journal.queue_view(crate::runtime::journal_product::view_record::StoredView::capture(binding.clone(),&session.core_state,active,&navigation));
                self.engines.mark_closed_view_checkpoint(id,self.journal.latest_view_sequence());
            } else if !self.journal.has_pending_view_for(&binding.stream) {
                drop(self.engines.finish_retiring(id));
            }
        }
    }

    pub(crate) fn poll_pending_window(&mut self) {
        if let Some(error) = self.journal.halt_reason().map(str::to_owned) {
            if let Some(mut pending) = self.pending_window.take() {
                let id = pending.engine;
                if let Some(completion) = pending.completion.take() {
                    completion.reply_window_create(Err(error.clone()));
                }
                self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    pending.origin,
                    "journal engine initialization failed",
                    error,
                );
                drop(pending);
                self.journal.abandon_halted_engine(id);
                drop(self.engines.retire_pending(id));
            }
            return;
        }
        let Some(pending) = self.pending_window.as_mut() else {
            return;
        };
        let id = pending.engine;
        if self.engines.journal_binding(id).is_none() {
            return;
        }
        let deadline = *pending.plugin_deadline.get_or_insert_with(|| {
            std::time::Instant::now() + crate::app::boot_machine::PLUGIN_WAIT_DEADLINE
        });
        let needed = self.boot_required_plugin_kinds();
        if !self.boot_pump_step_plugins_registered(&needed) && std::time::Instant::now() < deadline
        {
            return;
        }
        let Some(session) = self.engines.session_mut(id) else {
            if let Some(pending) = self.pending_window.take() {
                if let Some(completion) = pending.completion {
                    completion.reply_window_create(Err("opening engine disappeared".into()));
                }
                self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    pending.origin,
                    "journal engine initialization failed",
                    "opening engine disappeared",
                );
            }
            return;
        };
        let result = self.journal.poll_restore_bootstrap(session);
        if let Err(error) = result {
            if let Some(pending) = self.pending_window.take() {
                if let Some(completion) = pending.completion {
                    completion.reply_window_create(Err(error.clone()));
                }
                self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    pending.origin,
                    "journal engine initialization failed",
                    error,
                );
            }
            return;
        }
        if !self.journal.is_ready(id) {
            return;
        }
        let Some(pending) = self.pending_window.take() else {
            return;
        };
        let presentation = self.journal.take_restored_presentation(id);
        let mut state = match self.assemble_app_state(presentation) {
            Ok(state) => state,
            Err(error) => {
                if let Some(completion) = pending.completion {
                    completion.reply_window_create(Err(error.clone()));
                }
                self.notify_window_creation_failed(
                    WindowCreationTarget::NewWindow,
                    pending.origin,
                    "cannot assemble the committed engine View",
                    error,
                );
                return;
            }
        };
        if let Some(error) = pending.db_init_error {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                build_db_init_error_modal(&error),
            );
        }
        if let Some(theme) = pending.invalid_theme_name {
            crate::adapters::ui::info_modal::show_info_modal(
                &mut state,
                build_theme_fallback_modal(&theme),
            );
        }
        let window_id = pending.window.id();
        let behind = matches!(pending.origin, WindowRequestOrigin::Agent)
            .then(|| (pending.window.clone(), self.focused_main_winit()));
        self.register_window(pending.gpu, state, id, pending.window, pending.origin);
        if let Some((window, anchor)) = behind {
            show_agent_window(&window, anchor.as_deref());
            self.pending_focus_hint_clear.insert(window_id);
        }
        if let Some(completion) = pending.completion {
            completion.reply_window_create(Ok(u64::from(window_id)));
        }
    }
}
