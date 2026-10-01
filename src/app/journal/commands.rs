//! Accepted structural requests retain their original identity until a committed wire reply exists.
mod assembly;
mod category;
mod child;
mod close;
mod completion;
mod create;
mod create_spec;
mod display;
#[cfg(feature = "gui")]
mod divider;
#[cfg(feature = "gui")]
mod forwarding;
pub(crate) mod inbound;
mod intents;
pub(crate) mod live_resume;
mod notification;
mod replacement;
mod tab;
#[cfg(feature = "gui")]
mod view_completion;
mod wake;
mod workspace;
use super::*;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::runtime::journal_product::{Admission, ResponsePlan, StreamCommand};
use std::sync::mpsc::SyncSender;

const MAX_PENDING: usize = 64;

struct CategoryReservation {
    stream: String,
    name: String,
}

/// Live View continuation, never serialized with a structural command or replayed response.
#[derive(Clone)]
pub(crate) struct IntentViewContinuation {
    #[cfg(feature = "gui")]
    pub(crate) view: std::sync::Weak<()>,
    #[cfg(feature = "gui")]
    pub(crate) selection: std::sync::Weak<()>,
    pub(crate) activate_surface: Option<u32>,
    pub(crate) close_empty_engine: bool,
    #[cfg(feature = "gui")]
    pub(crate) preset_apply: bool,
    #[cfg(feature = "gui")]
    pub(crate) after_create: Option<crate::intent::CreateFollowup>,
    #[cfg(feature = "gui")]
    pub(crate) tutorial: Option<create::TutorialCreated>,
    #[cfg(feature = "gui")]
    pub(crate) tutorial_preparation: Option<std::sync::Weak<()>>,
    #[cfg(feature = "gui")]
    pub(crate) tutorial_surface: Option<(u32, crate::app::engine_action::SurfaceBinding)>,
}

enum Reply {
    Resume(live_resume::Resume),
    Remote(inbound::RemoteReply),
    #[cfg(feature = "gui")]
    Divider {
        engine: EngineId,
        sequence: u64,
        origin: crate::intent::IntentOrigin,
    },
    #[cfg(feature = "gui")]
    Settings {
        settings: Option<Box<crate::settings::Settings>>,
        generation: u64,
        origin: crate::intent::IntentOrigin,
    },
    Ipc(SyncSender<JsonRpcResponse>),
    Plugin {
        plugin_id: String,
        call_id: u64,
        binding: std::sync::Weak<()>,
    },
    Intent {
        engine: EngineId,
        origin: crate::intent::IntentOrigin,
        view: Option<IntentViewContinuation>,
    },
}

struct IntentResult {
    #[cfg(feature = "gui")]
    view: Option<IntentViewContinuation>,
    engine: EngineId,
    origin: crate::intent::IntentOrigin,
    response: JsonRpcResponse,
}

struct Pending {
    request: JsonRpcRequest,
    reply: Reply,
    queued: Option<Work>,
    needs_resolution: bool,
    category: Option<CategoryReservation>,
    resource: Option<create::Request>,
    #[cfg(feature = "gui")]
    forward: Option<super::forward::Draft>,
    /// Reservation follows the original request until its final reply, including effect-owned copies.
    #[cfg(feature = "gui")]
    forward_reserved: usize,
    one_shot_reserved: usize,
    closing: Option<close::Request>,
    replacing: Option<replacement::Request>,
    close_cause: close::Cause,
    waiting_command: Option<String>,
    activation_wait: Option<super::creation::ActivationReceipt>,
    created: Option<create::Completed>,
    bytes: usize,
    replay: bool,
    admitted: bool,
    retry_counted: bool,
    fixed: Option<Vec<StreamCommand>>,
    display: Option<display::DisplayContinuation>,
}

impl Pending {
    fn take_retry_outcome(
        &mut self,
        result: &Result<ResultValue, String>,
    ) -> Option<crate::ipc::handler::idempotency::RetryOutcome> {
        if self.retry_counted || self.request.idempotency_key.is_none() {
            return None;
        }
        let outcome = retry_outcome(result)?;
        self.retry_counted = true;
        Some(outcome)
    }
}

#[derive(Default)]
pub(super) struct Commands {
    pending: std::collections::BTreeMap<u64, Pending>,
    #[cfg(feature = "gui")]
    completed_dividers: Vec<(EngineId, u64, crate::intent::IntentOrigin, JsonRpcResponse)>,
    completed_plugins: Vec<(String, u64, std::sync::Weak<()>, JsonRpcResponse)>,
    completed_intents: Vec<IntentResult>,
    completed_live: Vec<(live_resume::Resume, JsonRpcResponse)>,
    completed_remote: Vec<(inbound::RemoteReply, JsonRpcResponse)>,
    completed_host_events: Vec<(EngineId, notification::Notification)>,
    #[cfg(feature = "gui")]
    settings_generation: u64,
    #[cfg(feature = "gui")]
    completed_settings: Vec<(
        u64,
        Option<Box<crate::settings::Settings>>,
        crate::intent::IntentOrigin,
        JsonRpcResponse,
    )>,
}

impl Commands {
    #[cfg(feature = "gui")]
    pub(super) fn has_remote_request(&self, engine: EngineId) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(&pending.reply,Reply::Remote(remote) if remote.engine==engine))
            || self
                .completed_remote
                .iter()
                .any(|(remote, _)| remote.engine == engine)
    }
    pub(super) fn has_closing(&self) -> bool {
        self.pending
            .values()
            .any(|pending| pending.closing.is_some() || pending.replacing.is_some())
    }
    #[cfg(feature = "gui")]
    pub(super) fn has_resource_request(&self, engine: EngineId) -> bool {
        self.completed_live
            .iter()
            .any(|(resume, _)| resume.engine == engine)
            || self.pending.values().any(|pending| {
                matches!(&pending.reply,Reply::Resume(resume) if resume.engine==engine)
                    || pending
                        .closing
                        .as_ref()
                        .is_some_and(|request| request.engine == engine)
                    || pending
                        .resource
                        .as_ref()
                        .is_some_and(|request| request.engine == engine)
                    || pending
                        .replacing
                        .as_ref()
                        .is_some_and(|request| request.engine == engine)
            })
    }

    fn deliver(&mut self, reply: Reply, response: JsonRpcResponse) {
        match reply {
            Reply::Resume(resume) => {
                if response.error.is_some() {
                    let (reply, response) = resume.error_reply(response);
                    self.deliver(reply, response);
                } else {
                    self.completed_live.push((resume, response));
                }
            }
            Reply::Remote(reply) => self.completed_remote.push((reply, response)),
            #[cfg(feature = "gui")]
            Reply::Divider {
                engine,
                sequence,
                origin,
            } => self
                .completed_dividers
                .push((engine, sequence, origin, response)),
            #[cfg(feature = "gui")]
            Reply::Settings {
                settings,
                generation,
                origin,
            } => {
                let settings = if response.error.is_some() {
                    None
                } else {
                    settings
                };
                self.completed_settings
                    .push((generation, settings, origin, response));
            }
            Reply::Ipc(sender) => crate::ipc::server::send_response(&sender, response),
            Reply::Plugin {
                plugin_id,
                call_id,
                binding,
            } => self
                .completed_plugins
                .push((plugin_id, call_id, binding, response)),
            Reply::Intent {
                engine,
                origin,
                view,
            } => self.completed_intents.push(IntentResult {
                #[cfg(feature = "gui")]
                view,
                engine,
                origin,
                response,
            }),
        }
    }
}

pub(crate) fn handles(method: &str) -> bool {
    tasty_ipc::method_meta::has_structure_journal_contract(method)
}

fn handles_request(request: &JsonRpcRequest) -> bool {
    handles(&request.method)
        || (request.method == "terminal.respawn"
            && request.params.get("cwd").is_some_and(|cwd| cwd.is_string()))
}

impl JournalApplication {
    /// Authentication and current permissions precede this call. No implicit target has been read.
    pub(crate) fn admit_ipc(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        caller: &crate::ipc::caller::CallerContext,
    ) -> bool {
        if !handles_request(&command.request) {
            return false;
        }
        self.admit_request(
            &command.request,
            Reply::Ipc(command.response_tx.clone()),
            crate::ipc::handler::idempotency::caller_scope(caller),
            "ipc",
        );
        true
    }

    fn admit_request(
        &mut self,
        request: &JsonRpcRequest,
        reply: Reply,
        scope: String,
        origin: &str,
    ) {
        let admission = Admission {
            key: request
                .idempotency_key
                .as_ref()
                .map(|key| tasty_event_store::CommandKey {
                    caller_scope: scope.clone(),
                    idempotency_key: key.clone(),
                }),
            original_digest: request_digest(request),
            actor: scope,
            origin: origin.into(),
            causation_id: None,
        };
        let work = Work::Admit(admission);
        let bytes = serde_json::to_vec(request)
            .expect("JSON request serializes")
            .len()
            + crate::runtime::journal_product::request_size(&work)
            + reply_weight(&reply);
        let held: usize = self
            .commands
            .pending
            .values()
            .map(|pending| pending.bytes)
            .sum::<usize>()
            .saturating_add(
                self.commands
                    .completed_live
                    .iter()
                    .map(|(resume, _)| resume.weight())
                    .sum::<usize>(),
            );
        if self.commands.pending.len() + self.commands.completed_live.len() >= MAX_PENDING
            || held.saturating_add(bytes) > tasty_ipc::admission::QUEUED_BYTES_LIMIT
        {
            self.commands.deliver(
                reply,
                JsonRpcResponse::error(
                    request.id.clone().unwrap_or_default(),
                    crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL,
                    "structure admission queue is full",
                ),
            );
            (self.wake)();
            return;
        }
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.commands.pending.insert(
            ticket,
            Pending {
                request: request.clone(),
                reply,
                queued: Some(work),
                needs_resolution: false,
                category: None,
                resource: None,
                #[cfg(feature = "gui")]
                forward: None,
                #[cfg(feature = "gui")]
                forward_reserved: 0,
                one_shot_reserved: 0,
                closing: None,
                replacing: None,
                close_cause: Default::default(),
                waiting_command: None,
                activation_wait: None,
                created: None,
                bytes,
                replay: false,
                admitted: false,
                retry_counted: false,
                fixed: None,
                display: None,
            },
        );
        (self.wake)();
    }

    pub(super) fn submit_commands(&mut self) -> Result<(), String> {
        let first = self.commands.pending.keys().next().copied();
        let mut oversized = Vec::new();
        for (ticket, pending) in self.commands.pending.iter_mut().take(MAX_PENDING) {
            let Some(work) = pending.queued.take() else {
                continue;
            };
            if !matches!(work, Work::Admit(_)) && first != Some(*ticket) {
                pending.queued = Some(work);
                continue;
            }
            // The worker returns the original work on pressure; no resource or input is cloned.
            match self.worker.submit_owned(Request {
                ticket: *ticket,
                work,
            }) {
                Ok(()) => {}
                Err((crate::runtime::journal_product::SubmitError::Busy, request)) => {
                    pending.queued = Some(request.work);
                    break;
                }
                Err((crate::runtime::journal_product::SubmitError::TooLarge, _)) => {
                    if pending.admitted {
                        pending.queued = Some(Work::CancelAdmission);
                    } else {
                        oversized.push(*ticket);
                    }
                }
                Err((error, _)) => return Err(format!("structure request submission: {error:?}")),
            }
        }
        for ticket in oversized {
            let pending = self
                .commands
                .pending
                .remove(&ticket)
                .expect("oversized request");
            self.commands.deliver(
                pending.reply,
                oversized_response(pending.request.id.unwrap_or_default()),
            );
        }
        Ok(())
    }

    fn refresh_command_weight(&mut self, ticket: u64) {
        let Some(pending) = self.commands.pending.get_mut(&ticket) else {
            return;
        };
        pending.bytes = pending_weight(pending);
        let total: usize = self
            .commands
            .pending
            .values()
            .map(|pending| pending.bytes)
            .sum::<usize>()
            .saturating_add(
                self.commands
                    .completed_live
                    .iter()
                    .map(|(resume, _)| resume.weight())
                    .sum::<usize>(),
            );
        if total > tasty_ipc::admission::QUEUED_BYTES_LIMIT {
            let response = JsonRpcResponse::error(
                serde_json::Value::Null,
                crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL,
                "resolved structure request exceeds the pending byte budget",
            );
            if !self.commands.pending[&ticket].admitted {
                let pending = self
                    .commands
                    .pending
                    .remove(&ticket)
                    .expect("sized command");
                let mut response = response;
                response.id = pending.request.id.unwrap_or_default();
                self.commands.deliver(pending.reply, response);
                (self.wake)();
                return;
            }
            let pending = self
                .commands
                .pending
                .get_mut(&ticket)
                .expect("sized command");
            pending.category = None;
            pending.resource = None;
            pending.fixed = None;
            pending.display = None;
            pending.request.params = serde_json::Value::Null;
            #[cfg(feature = "gui")]
            if let Reply::Settings { settings, .. } = &mut pending.reply {
                *settings = None;
            }
            pending.queued = Some(Work::Resolve {
                changes: Vec::new(),
                response: Some(ResponsePlan::Fixed(response)),
            });
            pending.bytes = pending_weight(pending);
        }
    }

    #[cfg(not(feature = "gui"))]
    pub(crate) fn creation_requests_needing_kind(&self) -> Vec<JsonRpcRequest> {
        self.commands
            .pending
            .values()
            .take(1)
            .filter(|pending| {
                pending.needs_resolution
                    && matches!(
                        pending.request.method.as_str(),
                        "workspace.create" | "tab.create" | "split"
                    )
            })
            .map(|pending| pending.request.clone())
            .collect()
    }

    pub(crate) fn requests_needing_resolution(&mut self) -> Vec<(u64, JsonRpcRequest)> {
        self.commands
            .pending
            .iter_mut()
            .take(1)
            .filter_map(|(ticket, pending)| {
                if !pending.needs_resolution {
                    return None;
                }
                pending.needs_resolution = false;
                Some((*ticket, pending.request.clone()))
            })
            .collect()
    }

    pub(crate) fn reject_resolved_request(&mut self, ticket: u64, mut response: JsonRpcResponse) {
        response.id = serde_json::Value::Null;
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.request.params = serde_json::Value::Null;
            pending.queued = Some(Work::Resolve {
                changes: Vec::new(),
                response: Some(ResponsePlan::Fixed(response)),
            });
            (self.wake)();
        }
        self.refresh_command_weight(ticket);
    }

    pub(crate) fn resolve_ipc_for_engine(&mut self, ticket: u64, session: &EngineSession) {
        if self.commands.pending.get(&ticket).is_some_and(|pending| {
            matches!(
                pending.request.method.as_str(),
                "surface.wake" | "intent.wake"
            )
        }) {
            self.resolve_wake(ticket, session);
            return;
        }
        if let Some(pending) = self.commands.pending.get(&ticket)
            && pending.request.method == "tab.move"
            && !matches!(pending.close_cause, close::Cause::RemoteHolder { .. })
        {
            let workspace = pending.request.params["pane_id"]
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .and_then(|pane| session.core_state.find_workspace_index_for_pane(pane))
                .and_then(|index| session.core_state.workspace_at(index));
            if let Some(workspace) = workspace
                && session
                    .live
                    .occupancy
                    .workspace_holder(workspace.id)
                    .is_some()
            {
                self.reject_resolved_request(
                    ticket,
                    crate::ipc::handler::hard_occupied_denial(
                        workspace.id,
                        &serde_json::Value::Null,
                    ),
                );
                return;
            }
        }
        if self.commands.pending.get(&ticket).is_some_and(|pending| {
            matches!(
                pending.request.method.as_str(),
                "intent.replace" | "intent.move-surface"
            )
        }) {
            self.resolve_replacement(ticket, session);
            return;
        }
        if self
            .commands
            .pending
            .get(&ticket)
            .is_some_and(|pending| pending.request.method == "intent.restore-closed")
        {
            self.resolve_undo(ticket, session);
            return;
        }
        if self.commands.pending.get(&ticket).is_some_and(|pending| {
            matches!(
                pending.request.method.as_str(),
                "terminal.kill"
                    | "workspace.close"
                    | "tab.close"
                    | "pane.close"
                    | "surface.close"
                    | "surface.close_self"
                    | "intent.close"
            )
        }) {
            self.resolve_close(ticket, session);
            return;
        }
        if self
            .commands
            .pending
            .get(&ticket)
            .is_some_and(|pending| pending.request.method == "intent.create")
        {
            self.resolve_fixed_creation(ticket, session);
            return;
        }
        let Some(pending) = self.commands.pending.get_mut(&ticket) else {
            return;
        };
        let Some(binding) = session.journal_binding.as_ref() else {
            self.reject_resolved_request(
                ticket,
                JsonRpcResponse::internal_error(
                    serde_json::Value::Null,
                    "engine has no journal binding",
                ),
            );
            return;
        };
        if matches!(
            pending.request.method.as_str(),
            "workspace.update"
                | "workspace.move"
                | "intent.workspace-mapping"
                | "intent.workspace-rename"
                | "intent.tab-name"
                | "intent.tab-move"
                | "tab.move"
        ) {
            match if pending.request.method == "tab.move" {
                tab::move_public(&pending.request, session)
            } else if pending.request.method == "intent.tab-name" {
                tab::rename(&pending.request, session)
            } else if pending.request.method == "intent.tab-move" {
                tab::move_tab(&pending.request)
            } else {
                workspace::resolve(&pending.request, session, &binding.stream)
            } {
                Ok(resolved) => {
                    pending.display = resolved.display;
                    pending.queued = Some(Work::Resolve {
                        changes: resolved
                            .changes
                            .into_iter()
                            .map(|command| StreamCommand {
                                stream: binding.stream.clone(),
                                command,
                            })
                            .collect(),
                        response: Some(resolved.response),
                    });
                }
                Err(response) => self.reject_resolved_request(ticket, response),
            }
        } else {
            match category::resolve(&pending.request, &session.core_state) {
                Ok(category::Resolved::Create(name)) => {
                    pending.category = Some(CategoryReservation {
                        stream: binding.stream.clone(),
                        name,
                    });
                    pending.queued = Some(Work::Reserve(vec![(tasty_core::IdKind::Category, 1)]));
                }
                Ok(category::Resolved::Apply(command, response)) => {
                    pending.queued = Some(Work::Resolve {
                        changes: vec![StreamCommand {
                            stream: binding.stream.clone(),
                            command,
                        }],
                        response: Some(ResponsePlan::Fixed(response)),
                    });
                }
                Err(response) => self.reject_resolved_request(ticket, response),
            }
        }
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            // After the miss fixes its input, the original digest belongs to the worker. Keeping
            // another params clone here would duplicate the resolved command and frozen reply.
            pending.request.params = serde_json::Value::Null;
        }
        self.refresh_command_weight(ticket);
        (self.wake)();
    }

    pub(super) fn fail_pending_commands(&mut self, reason: &str) {
        for (_, pending) in std::mem::take(&mut self.commands.pending) {
            self.commands.deliver(
                pending.reply,
                JsonRpcResponse::internal_error(pending.request.id.unwrap_or_default(), reason),
            );
        }
    }
}

impl JournalApplication {
    pub(crate) fn take_changed_engines(&mut self) -> Vec<EngineId> {
        self.changed_engines.drain().collect()
    }

    #[cfg(not(feature = "gui"))]
    fn reject_headless_owner(
        &mut self,
        ticket: u64,
        request: &JsonRpcRequest,
        session: &EngineSession,
    ) -> bool {
        if self.commands.pending.get(&ticket).is_some_and(|pending| matches!(&pending.reply, Reply::Intent { engine, .. } if *engine != session.id)) {
        self.reject_resolved_request(ticket, JsonRpcResponse::invalid_params(serde_json::Value::Null, "intent engine no longer exists"));
        return true;
    }

        let named =
            crate::core::request_target::request_resource_id(&request.method, &request.params);
        if let Some(resource) = named
            && !crate::core::request_target::engine_has_resource(&session.as_ref(), resource)
        {
            self.reject_resolved_request(
                ticket,
                JsonRpcResponse::invalid_params(
                    serde_json::Value::Null,
                    crate::core::request_target::unowned_target_message(resource, &request.method),
                ),
            );
            return true;
        }
        false
    }

    #[cfg(not(feature = "gui"))]
    pub(crate) fn resolve_headless_requests(
        &mut self,
        session: &mut EngineSession,
        state: &mut crate::state::RequestContext,
        services: &crate::app::services::AppServices,
    ) {
        for (ticket, request) in self.requests_needing_resolution() {
            if self.reject_headless_owner(ticket, &request, session) {
                continue;
            }
            if request.method == "remote.structural" {
                let missing = self.unresolved_remote_anchor(ticket, [session.as_ref()]);
                self.resolve_remote_request(ticket, session, services, missing);
                continue;
            }
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
                self.resolve_public_creation(ticket, session, services);
                continue;
            }
            if matches!(
                request.method.as_str(),
                "preset.apply" | "intent.preset-apply"
            ) {
                self.resolve_preset(
                    ticket,
                    session,
                    services,
                    Some(state.focused_pane_id(&session.core_state)),
                );
                continue;
            }
            if request.method == "workspace.create" {
                let kind = request
                    .params
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("terminal");
                match crate::ipc::handler::workspace::resolve_create_cwd(
                    &request.params,
                    kind,
                    &crate::ipc::request_scope::RequestScope::capture(state, &session.core_state),
                    &session.as_ref(),
                    &serde_json::Value::Null,
                ) {
                    Ok(cwd) => self.resolve_workspace_creation(ticket, session, cwd),
                    Err(response) => self.reject_resolved_request(ticket, response),
                }
            } else {
                self.resolve_ipc_for_engine(ticket, session);
            }
        }
        for (engine, replacement) in std::mem::take(&mut self.replacements) {
            if engine == session.id {
                state.navigation.apply_replacement(replacement);
            }
        }
        for (engine, event) in std::mem::take(&mut self.commands.completed_host_events) {
            if engine == session.id
                && let Some(event) = event.resolve(&session.as_ref(), &state.navigation)
            {
                session.borrow_mut().enqueue_host_event(event);
            }
        }
        for result in std::mem::take(&mut self.commands.completed_intents) {
            if let Some(error) = result.response.error {
                tracing::warn!(engine = ?result.engine, origin = ?result.origin, "headless journal intent failed: {}", error.message);
            }
        }
    }
}

fn retry_outcome(
    result: &Result<ResultValue, String>,
) -> Option<crate::ipc::handler::idempotency::RetryOutcome> {
    use crate::ipc::handler::idempotency::RetryOutcome as O;
    match result {
        Ok(ResultValue::NeedsResolution) => Some(O::Executed),
        Ok(ResultValue::JoinedAdmission { .. }) => Some(O::InFlight),
        Ok(ResultValue::Stored(_) | ResultValue::RecoveryRequired { replay: true, .. }) => {
            Some(O::Replayed)
        }
        Err(error) if error.contains("idempotency key") => Some(O::Conflicted),
        _ => None,
    }
}

fn oversized_response(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::error(
        id,
        crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG,
        "structural request exceeds the accepted payload or queue byte limit",
    )
}

impl JournalApplication {
    pub(crate) fn admit_plugin(
        &mut self,
        request: &JsonRpcRequest,
        caller: &crate::ipc::caller::CallerContext,
        call: &tasty_host_plugin::manager::PendingPluginCall,
        manager: Option<&crate::plugin::PluginManager>,
    ) -> bool {
        // This runs only after namespace routing. The owning image plugin trampolines
        // into the host; the outer namespace request retains its existing key contract.
        let host_conversion = cfg!(feature = "gui") && request.method == "image.open";
        if !handles_request(request) && !host_conversion {
            return false;
        }
        if !manager.is_some_and(|manager| manager.plugin_call_is_current(call)) {
            tracing::warn!(plugin=%call.plugin_id,"discarding structural call from retired plugin");
            return true;
        }
        self.admit_request(
            request,
            Reply::Plugin {
                plugin_id: call.plugin_id.clone(),
                call_id: call.call_id,
                binding: call.binding.clone(),
            },
            crate::ipc::handler::idempotency::caller_scope(caller),
            "plugin",
        );
        true
    }

    #[cfg(feature = "gui")]
    pub(crate) fn admit_host_fallback_ipc(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        caller: &crate::ipc::caller::CallerContext,
    ) -> bool {
        if command.request.method != "image.open" {
            return false;
        }
        self.admit_request(
            &command.request,
            Reply::Ipc(command.response_tx.clone()),
            crate::ipc::handler::idempotency::caller_scope(caller),
            "ipc",
        );
        true
    }

    pub(crate) fn deliver_plugin_replies(
        &mut self,
        manager: Option<&mut crate::plugin::PluginManager>,
    ) {
        let replies = std::mem::take(&mut self.commands.completed_plugins);
        let Some(manager) = manager else {
            return;
        };
        for (plugin, call, binding, response) in replies {
            if !manager.send_bound_ipc_result(&plugin, &binding, call, response) {
                tracing::warn!("discarding structural reply for retired plugin {plugin}");
            }
        }
    }
}

fn pending_weight(pending: &Pending) -> usize {
    #[cfg(feature = "gui")]
    let forward_bytes = pending.forward_reserved.max(
        pending
            .forward
            .as_ref()
            .map_or(0, super::forward::Draft::weight),
    );
    #[cfg(not(feature = "gui"))]
    let forward_bytes: usize = 0;
    forward_bytes
        .saturating_add(pending.one_shot_reserved)
        .saturating_add(
            serde_json::to_vec(&pending.request)
                .expect("request JSON serializes")
                .len()
                .saturating_add(
                    pending
                        .queued
                        .as_ref()
                        .map_or(0, crate::runtime::journal_product::request_size),
                )
                .saturating_add(pending.fixed.as_ref().map_or(0, |commands| {
                    serde_json::to_vec(commands)
                        .expect("fixed commands serialize")
                        .len()
                }))
                .saturating_add(
                    pending
                        .display
                        .as_ref()
                        .map_or(0, display::DisplayContinuation::weight),
                )
                .saturating_add(pending.resource.as_ref().map_or(0, create::Request::weight))
                .saturating_add(pending.closing.as_ref().map_or(0, close::Request::weight))
                .saturating_add(
                    pending
                        .replacing
                        .as_ref()
                        .map_or(0, replacement::Request::weight),
                )
                .saturating_add(pending.waiting_command.as_ref().map_or(0, String::len))
                .saturating_add(
                    pending
                        .created
                        .as_ref()
                        .map_or(0, create::Completed::weight),
                )
                .saturating_add(reply_weight(&pending.reply))
                .saturating_add(
                    pending
                        .category
                        .as_ref()
                        .map_or(0, |category| category.name.len() + category.stream.len()),
                ),
        )
}

fn reply_weight(_reply: &Reply) -> usize {
    if let Reply::Resume(resume) = _reply {
        return resume.weight();
    }
    if let Reply::Remote(_) = _reply {
        return std::mem::size_of::<inbound::RemoteReply>();
    }
    #[cfg(feature = "gui")]
    if let Reply::Settings { settings, .. } = _reply {
        return serde_json::to_vec(settings)
            .expect("settings serialize")
            .len();
    }
    0
}

#[cfg(test)]
mod tests;

// Stream JSON through a hash so raw terminal input is never retained as a command digest.
fn request_digest(request: &JsonRpcRequest) -> Vec<u8> {
    // Existing keys retain their original comparison representation. Only the newly journaled
    // raw-input composite uses a hash; upgrading must not turn old Stored replies into conflicts.
    if !matches!(
        request.method.as_str(),
        "terminal.spawn" | "terminal.respawn"
    ) {
        return serde_json::to_vec(&(&request.method, &request.params))
            .expect("JSON request serializes");
    }
    use sha2::Digest;
    struct HashWriter(sha2::Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(sha2::Sha256::new());
    serde_json::to_writer(&mut writer, &(&request.method, &request.params))
        .expect("JSON request serializes");
    writer.0.finalize().to_vec()
}
