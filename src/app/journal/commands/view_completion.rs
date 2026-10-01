//! Resolve fixed GUI requests and deliver their original live View continuations.
use super::*;

#[cfg(feature = "gui")]
impl crate::app::App {
    pub(crate) fn resolve_journal_requests(&mut self) {
        self.execute_journal_forwards();
        for (ticket, request) in self.journal.requests_needing_resolution() {
            if let Some(id) = self
                .journal
                .commands
                .pending
                .get(&ticket)
                .and_then(|pending| match &pending.reply {
                    Reply::Intent { engine, .. } => Some(*engine),
                    Reply::Remote(reply) => Some(reply.engine),
                    Reply::Resume(resume) => Some(resume.engine),
                    _ => None,
                })
            {
                if self.try_resolve_mirror_request(ticket, id, &request) {
                    continue;
                }
                if let Some(session) = self.engines.session_mut(id) {
                    if request.method == "remote.structural" {
                        self.journal
                            .resolve_remote_request(ticket, session, &self.services);
                    } else if request.method == "intent.preset-apply" {
                        self.journal
                            .resolve_preset(ticket, session, &self.services, None);
                    } else {
                        self.journal.resolve_ipc_for_engine(ticket, session);
                    }
                } else {
                    self.journal.reject_resolved_request(
                        ticket,
                        JsonRpcResponse::invalid_params(
                            serde_json::Value::Null,
                            "intent engine no longer exists",
                        ),
                    );
                }
                continue;
            }
            let named =
                crate::core::request_target::request_resource_id(&request.method, &request.params);
            let window = match self.find_request_owner(&request.method, &request.params) {
                Ok(found) if named.is_some() => found,
                Ok(found) => found.or(self.view.focused_view_id),
                Err(error) => {
                    self.journal.reject_resolved_request(
                        ticket,
                        JsonRpcResponse::invalid_params(serde_json::Value::Null, error),
                    );
                    continue;
                }
            };
            let id = window
                .and_then(|window| self.engines.of_window(window))
                .or_else(|| {
                    self.engines()
                        .parked_with_ids()
                        .find(|(_, engine)| {
                            named.is_none_or(|resource| {
                                crate::core::request_target::engine_has_resource(engine, resource)
                            })
                        })
                        .map(|(id, _)| id)
                });
            if let Some(engine) = id
                && self.try_resolve_mirror_request(ticket, engine, &request)
            {
                continue;
            }
            let preset_pane = if request.method == "preset.apply" {
                id.and_then(|engine| self.engines_mut().resolve(engine))
                    .map(|context| context.state.focused_pane_id(&context.engine.read()))
            } else {
                None
            };
            let creation_cwd = if request.method == "workspace.create" {
                id.and_then(|engine| self.engines_mut().resolve(engine))
                    .map(|context| {
                        let kind = request
                            .params
                            .get("type")
                            .and_then(|value| value.as_str())
                            .unwrap_or("terminal");
                        crate::ipc::handler::workspace::resolve_create_cwd(
                            &request.params,
                            kind,
                            &crate::ipc::request_scope::RequestScope::capture(
                                context.state,
                                context.engine.core,
                                None,
                            ),
                            &context.engine.as_ref(),
                            &serde_json::Value::Null,
                        )
                    })
            } else {
                None
            };
            let Some(session) = id.and_then(|id| self.engines.session_mut(id)) else {
                let response = match named {
                    Some(resource) => JsonRpcResponse::invalid_params(
                        serde_json::Value::Null,
                        crate::core::request_target::unowned_target_message(
                            resource,
                            &request.method,
                        ),
                    ),
                    None => JsonRpcResponse::invalid_params(
                        serde_json::Value::Null,
                        "no engine is available for structural request",
                    ),
                };
                self.journal.reject_resolved_request(ticket, response);
                continue;
            };
            if matches!(
                request.method.as_str(),
                "terminal.spawn"
                    | "terminal.respawn"
                    | "pty.attach_surface"
                    | "tab.create"
                    | "split"
                    | "surface.respawn_terminal"
                    | "image.open"
            ) {
                self.journal
                    .resolve_public_creation(ticket, session, &self.services);
                continue;
            }
            if request.method == "preset.apply" {
                self.journal
                    .resolve_preset(ticket, session, &self.services, preset_pane);
                continue;
            }
            if let Some(cwd) = creation_cwd {
                match cwd {
                    Ok(cwd) => self
                        .journal
                        .resolve_workspace_creation(ticket, session, cwd),
                    Err(response) => self.journal.reject_resolved_request(ticket, response),
                }
            } else {
                self.journal.resolve_ipc_for_engine(ticket, session);
            }
        }
        self.finish_live_inputs();
        self.finish_preset_captures();
        self.deliver_remote_journal_results();
        self.journal
            .deliver_plugin_replies(self.plugin_manager.as_mut());
        for (settings, origin, response) in self.journal.take_settings_results() {
            if let Some(error) = response.error {
                if let Some((state, engine)) = self.engines_mut().sessions().next() {
                    crate::intent::report_apply_error(
                        state,
                        engine.core,
                        &origin,
                        "journal category reset",
                        &anyhow::anyhow!(error.message),
                    );
                }
            } else {
                self.apply_settings_after_structure(
                    *settings.expect("successful settings continuation"),
                );
            }
        }
        for (engine, sequence, origin, response) in
            std::mem::take(&mut self.journal.commands.completed_dividers)
        {
            if let Some(context) = self.engines_mut().resolve(engine) {
                context.state.layout_previews.cancel(sequence);
                if let Some(error) = response.error {
                    crate::intent::report_apply_error(
                        context.state,
                        context.engine.core,
                        &origin,
                        "divider commit",
                        &anyhow::anyhow!(error.message),
                    );
                }
                if let Some(view) = context.view {
                    view.mark_dirty();
                }
            }
        }
        let mut presentations: std::collections::HashMap<_, _> = self
            .engines()
            .window_pairs()
            .filter_map(|(window, main, engine)| {
                self.engines.of_window(window).map(|id| {
                    (
                        id,
                        crate::model::StructurePresentationSnapshot::capture(
                            &engine.workspaces(),
                            &engine.categories(),
                            &main.state.navigation,
                        ),
                    )
                })
            })
            .collect();
        for (id, state, engine) in self.engines.parked_sessions() {
            presentations.insert(
                id,
                crate::model::StructurePresentationSnapshot::capture(
                    &engine.workspaces(),
                    &engine.categories(),
                    &state.navigation,
                ),
            );
        }
        for (id, navigation, _) in self.engines.preserved_closes() {
            if let Some(engine) = self.engines.get(id) {
                presentations.insert(
                    id,
                    crate::model::StructurePresentationSnapshot::capture(
                        &engine.workspaces(),
                        &engine.categories(),
                        &navigation,
                    ),
                );
            }
        }
        for (engine, event) in std::mem::take(&mut self.journal.commands.completed_host_events) {
            if let Some(session) = self.engines.session_mut(engine) {
                let presentation = presentations
                    .get(&engine)
                    .unwrap_or(&session.remote.presentation);
                if let Some(event) = event.resolve(&session.as_ref(), presentation) {
                    session.borrow_mut().enqueue_host_event(event);
                }
            }
        }
        for mut result in std::mem::take(&mut self.journal.commands.completed_intents) {
            if result.response.error.is_none()
                && let Some(surface) = result
                    .response
                    .result
                    .as_ref()
                    .and_then(|value| value.get("restored_surface_id"))
                    .and_then(|value| value.as_u64())
                    .and_then(|id| u32::try_from(id).ok())
                && let Some(view) = result.view.as_mut()
            {
                view.activate_surface = Some(surface);
            }
            if result.origin.is_user()
                && !result.response.idempotent_replay
                && let Some(continuation) = result.view.as_ref()
                && let Some(ticket) = continuation.tutorial_preparation.as_ref()
                && let Some(context) = self.engines_mut().resolve(result.engine)
                && context
                    .view
                    .as_ref()
                    .is_some_and(|view| view.state.matches_identity(&continuation.view))
                && context.state.tutorial.matches_preparation(ticket)
            {
                let practice = continuation
                    .tutorial_surface
                    .as_ref()
                    .filter(|(_, target)| {
                        result.response.error.is_none() && target.current(&context.engine.as_ref())
                    })
                    .and_then(|(surface, _)| {
                        let (index, pane) =
                            context.engine.find_workspace_index_for_surface(*surface)?;
                        let workspace = context.engine.workspace_at(index)?.id;
                        let tab = context.engine.find_tab_for_surface(*surface)?;
                        Some(crate::adapters::ui::tutorial::PracticeContext {
                            workspace,
                            pane,
                            tab,
                        })
                    });
                if let Some(practice) = practice {
                    context.state.tutorial.prepared(practice);
                } else {
                    context.state.tutorial.preparation_failed();
                }
                if let Some(view) = context.view {
                    view.mark_dirty();
                }
            }
            if result.response.error.is_none()
                && result.origin.is_user()
                && !result.response.idempotent_replay
                && let Some(continuation) = result.view.as_ref()
                && let Some(tutorial) = continuation.tutorial.as_ref()
                && let Some(context) = self.engines_mut().resolve(result.engine)
                && context
                    .view
                    .as_ref()
                    .is_some_and(|view| view.state.matches_identity(&continuation.view))
            {
                tutorial.observe(context.state, context.engine.core);
            }
            if result.response.error.is_none()
                && result.origin.is_user()
                && let Some(continuation) = result.view.as_ref()
                && continuation.close_empty_engine
                && let Some(context) = self.engines_mut().resolve(result.engine)
                && context.engine.workspaces().is_empty()
                && let Some(view) = context.view
                && view.state.matches_identity(&continuation.view)
            {
                view.state.close_requested = true;
            }
            if result.response.error.is_none()
                && result.origin.is_user()
                && let Some(continuation) = result.view
                && let Some(surface) = continuation.activate_surface
                && let Some(context) = self.engines_mut().resolve(result.engine)
                && context
                    .view
                    .as_ref()
                    .is_some_and(|view| view.state.matches_identity(&continuation.view))
                && context
                    .state
                    .navigation
                    .matches_generation(&continuation.selection)
                && let Some((index, pane_id)) =
                    context.engine.find_workspace_index_for_surface(surface)
                && let Some(workspace) = context.engine.workspace_at(index)
                && let Some(pane) = workspace.pane_layout().find_pane(pane_id)
                && let Some(tab) = pane.tabs.iter().find(|tab| tab.contains_surface(surface))
            {
                if let Some(saved) = result
                    .response
                    .result
                    .as_ref()
                    .and_then(|value| value.get("presentation"))
                    .and_then(|value| {
                        serde_json::from_value::<tasty_core::UndoPresentation>(value.clone()).ok()
                    })
                {
                    for (workspace, pane) in saved.focused_panes {
                        if let Some(workspace) = context
                            .engine
                            .find_workspace_index_for_id(workspace)
                            .and_then(|index| context.engine.workspace_at(index))
                        {
                            context.state.navigation.select_pane(workspace, pane);
                        }
                    }
                    for (pane, tab) in saved.selected_tabs {
                        if let Some(pane) = context.engine.find_pane_by_id(pane) {
                            context.state.navigation.select_tab(pane, tab);
                        }
                    }
                    for (tab, surface) in saved.selected_surfaces {
                        if let Some(tab) = context
                            .engine
                            .find_pane_for_tab(tab)
                            .and_then(|pane| context.engine.find_pane_by_id(pane))
                            .and_then(|pane| pane.tabs.iter().find(|candidate| candidate.id == tab))
                        {
                            context.state.navigation.select_surface(tab, surface);
                        }
                    }
                }
                context
                    .state
                    .navigation
                    .select_workspace(&context.engine.workspaces(), workspace.id);
                context.state.navigation.select_pane(workspace, pane.id);
                context.state.navigation.select_tab(pane, tab.id);
                context.state.navigation.select_surface(tab, surface);
                if let Some(followup) = continuation.after_create {
                    match followup {
                        crate::intent::CreateFollowup::Prompt { kind } => {
                            context.state.enqueue_convert_input_popup(
                                &context.engine.read(),
                                &kind,
                                Some(surface),
                            );
                        }
                    }
                }
                if let Some(view) = context.view {
                    view.mark_dirty();
                }
            }
            if let Some(error) = result.response.error
                && let Some(context) = self.engines_mut().resolve(result.engine)
            {
                crate::intent::report_apply_error(
                    context.state,
                    context.engine.core,
                    &result.origin,
                    "journal structural intent",
                    &anyhow::anyhow!(error.message),
                );
            }
        }
        for (engine, replacement) in std::mem::take(&mut self.journal.replacements) {
            if let Some(context) = self.engines_mut().resolve(engine) {
                context.state.navigation.apply_replacement(replacement);
            }
        }
        for id in self.journal.take_changed_engines() {
            if let Some(mut context) = self.engines_mut().resolve(id) {
                context.state.reconcile_presentation(&context.engine);
                context
                    .engine
                    .refresh_attach_presentation(&context.state.navigation);
                if let Some(view) = context.view {
                    view.mark_dirty();
                }
            }
        }
    }
}
