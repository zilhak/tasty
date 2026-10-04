//! Nonblocking preset capture entry points; saved data outlives its originating View.
use super::{App, journal::PresetCaptureReply};
use crate::ipc::{
    protocol::JsonRpcResponse,
    server::{IpcCommand, send_response},
};
#[cfg(not(feature = "gui"))]
use crate::runtime::engine_session::EngineSession;
#[cfg(feature = "gui")]
use crate::view::ui::View;
impl App {
    #[cfg(not(feature = "gui"))]
    pub(crate) fn preset_capture_on_session(
        &mut self,
        command: &IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
        session: &EngineSession,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> bool {
        if command.request.method != "preset.capture" {
            return false;
        }
        if let Some(handled) = crate::ipc::handler::idempotency::run_app_layer(
            checked.caller(),
            command,
            true,
            |handled| *handled,
            |relayed| self.preset_capture_session_miss(relayed, session, presentation),
        ) {
            return handled;
        }
        self.preset_capture_session_miss(command, session, presentation)
    }
    #[cfg(not(feature = "gui"))]
    fn preset_capture_session_miss(
        &mut self,
        command: &IpcCommand,
        session: &EngineSession,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> bool {
        let id = command.request.id.clone().unwrap_or_default();
        let (kind, source, name) =
            match crate::ipc::handler::preset::decode_capture_request(&command.request.params, &id)
            {
                Ok(request) => request,
                Err(response) => {
                    send_response(&command.response_tx, response);
                    return true;
                }
            };
        let reply = PresetCaptureReply::Ipc {
            id: id.clone(),
            sender: command.response_tx.clone(),
            name,
        };
        if let Err(error) =
            self.journal
                .queue_preset_capture(session, presentation, kind, source, reply)
        {
            send_response(
                &command.response_tx,
                JsonRpcResponse::invalid_params(id, error),
            );
        }
        true
    }
    pub(crate) fn finish_preset_captures(&mut self) {
        for completion in self.journal.take_preset_captures() {
            match completion.save(&self.services) {
                #[cfg(feature = "gui")]
                Some(super::journal::PresetCaptureOutput::Intent(notice)) => {
                    #[cfg(feature = "gui")]
                    self.apply_preset_capture_notice(notice);
                }
                Some(super::journal::PresetCaptureOutput::Plugin(reply)) => {
                    if let Some(manager) = self.plugin_manager.as_mut()
                        && !manager.send_bound_ipc_result(
                            &reply.plugin,
                            &reply.binding,
                            reply.call_id,
                            reply.response,
                        )
                    {
                        tracing::warn!(plugin=%reply.plugin,"discarding preset result for retired plugin");
                    }
                }
                None => {}
            }
        }
    }
}
#[cfg(feature = "gui")]
impl App {
    pub(crate) fn defer_preset_capture(
        &mut self,
        command: &IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
    ) -> bool {
        if command.request.method != "preset.capture" {
            return false;
        }
        if let Some(handled) = crate::ipc::handler::idempotency::run_app_layer(
            checked.caller(),
            command,
            true,
            |handled| *handled,
            |relayed| self.preset_capture_miss(relayed),
        ) {
            return handled;
        }
        self.preset_capture_miss(command)
    }
    fn preset_capture_miss(&mut self, command: &IpcCommand) -> bool {
        let id = command.request.id.clone().unwrap_or_default();
        let (kind, source, name) =
            match crate::ipc::handler::preset::decode_capture_request(&command.request.params, &id)
            {
                Ok(request) => request,
                Err(response) => {
                    send_response(&command.response_tx, response);
                    return true;
                }
            };
        let owns = |engine: &crate::core::CoreState| match kind {
            tasty_presets::PresetKind::Workspace => engine
                .workspaces()
                .into_iter()
                .any(|workspace| workspace.id == source),
            tasty_presets::PresetKind::Pane => engine.find_pane_by_id(source).is_some(),
            tasty_presets::PresetKind::Tab => engine.find_pane_for_tab(source).is_some(),
        };
        let presentation = self
            .engines()
            .sessions()
            .find(|(_, engine)| owns(engine.core))
            .map(|(state, engine)| {
                crate::model::StructurePresentationSnapshot::capture(
                    engine.workspaces(),
                    engine.categories(),
                    &state.navigation,
                )
            });
        let session = self
            .engines
            .all_sessions()
            .find(|session| owns(&session.core_state));
        let result = match (session, presentation) {
            (Some(session), Some(presentation)) => self.journal.queue_preset_capture(
                session,
                &presentation,
                kind,
                source,
                PresetCaptureReply::Ipc {
                    id: id.clone(),
                    sender: command.response_tx.clone(),
                    name,
                },
            ),
            _ => Err(format!(
                "{} id {source} not found",
                match kind {
                    tasty_presets::PresetKind::Workspace => "Workspace",
                    tasty_presets::PresetKind::Pane => "Pane",
                    tasty_presets::PresetKind::Tab => "Tab",
                }
            )),
        };
        if let Err(error) = result {
            send_response(
                &command.response_tx,
                JsonRpcResponse::invalid_params(id, error),
            );
        }
        true
    }
    pub(crate) fn queue_preset_capture_intent(
        &mut self,
        engine: crate::runtime::engine_session::EngineId,
        kind: tasty_presets::PresetKind,
        source: u32,
        presentation: &crate::model::StructurePresentationSnapshot,
        origin: &crate::intent::IntentOrigin,
    ) -> Result<(), String> {
        let view = self
            .engines
            .window_of(engine)
            .and_then(|window| self.view.views.get(&window))
            .and_then(|view| view.as_main())
            .map(|view| view.base.state.identity())
            .unwrap_or_default();
        let session = self
            .engines
            .all_sessions()
            .find(|session| session.id == engine)
            .ok_or("preset capture engine disappeared")?;
        self.journal
            .queue_preset_capture(
                session,
                presentation,
                kind,
                source,
                PresetCaptureReply::Intent {
                    origin: origin.clone(),
                    view,
                    name: None,
                },
            )
            .map(|_| ())
    }
    fn apply_preset_capture_notice(&mut self, notice: super::journal::PresetCaptureNotice) {
        let Some(binding) = self.engines.journal_binding(notice.engine) else {
            return;
        };
        if binding.journal_id != notice.binding.journal_id
            || binding.stream != notice.binding.stream
            || binding.incarnation != notice.binding.incarnation
            || binding.runtime_epoch != notice.binding.runtime_epoch
        {
            return;
        }
        let Some(window) = self.engines.window_of(notice.engine) else {
            return;
        };
        let Some(main) = self
            .view
            .views
            .get_mut(&window)
            .and_then(|view| view.as_main_mut())
        else {
            return;
        };
        if !main.base.state.matches_identity(&notice.view) {
            return;
        }
        match notice.result {
            Ok((kind, name)) => {
                let key = match kind {
                    tasty_presets::PresetKind::Workspace => "preset.toast.saved_workspace",
                    tasty_presets::PresetKind::Tab => "preset.toast.saved_tab",
                    tasty_presets::PresetKind::Pane => "preset.toast.saved_pane",
                };
                main.state.toasts.push(
                    crate::i18n::t(key),
                    crate::model::toast_kind::ToastKind::Info,
                    crate::model::toast_kind::ToastScope::Window,
                );
                if notice.origin.is_user() {
                    main.state.dialogs.pending_open_preset_window = true;
                    main.state.dialogs.pending_preset_window_selection = Some((kind, name));
                }
            }
            Err(error) => {
                tracing::warn!(%error,"preset capture failed");
                if notice.origin.is_user() {
                    main.state.toasts.push(
                        crate::i18n::t("preset.toast.save_failed"),
                        crate::model::toast_kind::ToastKind::Error,
                        crate::model::toast_kind::ToastScope::Window,
                    );
                }
            }
        }
        main.mark_dirty();
    }
}
