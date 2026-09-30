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
        let mut sessions: Vec<_> = self.engines.all_sessions_mut().collect();
        if let Err(error) = self.journal.poll_bootstrap(&mut sessions) {
            tracing::error!("journal publication halted: {error}");
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
        let Some(pending) = self.pending_window.as_ref() else {
            return;
        };
        let id = pending.engine;
        if self.engines.journal_binding(id).is_none() {
            return;
        }
        let deadline = *self
            .pending_window
            .as_mut()
            .expect("opening window")
            .plugin_deadline
            .get_or_insert_with(|| {
                std::time::Instant::now() + crate::app::boot_machine::PLUGIN_WAIT_DEADLINE
            });
        let needed = self.boot_required_plugin_kinds();
        if !self.boot_pump_step_plugins_registered(&needed) && std::time::Instant::now() < deadline
        {
            return;
        }
        let session = self
            .engines
            .session_mut(id)
            .expect("opening engine remains registered");
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
        let pending = self.pending_window.take().expect("opening window");
        let presentation = self.journal.take_restored_presentation(id);
        let mut state = self.assemble_app_state(presentation);
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
