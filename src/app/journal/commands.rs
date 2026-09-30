//! Accepted structural requests retain their original identity until a committed wire reply exists.
mod category;
mod display;
#[cfg(feature = "gui")]
mod divider;
mod intents;
mod notification;
mod tab;
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

enum Reply {
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
    },
}

struct IntentResult {
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
    fn deliver(&mut self, reply: Reply, response: JsonRpcResponse) {
        match reply {
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
            Reply::Intent { engine, origin } => self.completed_intents.push(IntentResult {
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

impl JournalApplication {
    /// Authentication and current permissions precede this call. No implicit target has been read.
    pub(crate) fn admit_ipc(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        caller: &crate::ipc::caller::CallerContext,
    ) -> bool {
        if !handles(&command.request.method) {
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
            original_digest: serde_json::to_vec(&(&request.method, &request.params))
                .expect("JSON request serializes"),
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
            .sum();
        if self.commands.pending.len() >= MAX_PENDING
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
            .sum();
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

    pub(crate) fn requests_needing_resolution(&mut self) -> Vec<(u64, JsonRpcRequest)> {
        self.commands
            .pending
            .iter_mut()
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
        ) {
            match if pending.request.method == "intent.tab-name" {
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
                    pending.queued = Some(Work::Reserve(vec![(tasty_domain::IdKind::Category, 1)]));
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

    pub(super) fn answer_command(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
        sessions: &mut [&mut EngineSession],
    ) -> Result<bool, String> {
        let Some(pending) = self.commands.pending.get_mut(&ticket) else {
            return Ok(false);
        };
        if let Some(outcome) = pending.take_retry_outcome(result) {
            crate::ipc::handler::idempotency::note_journal_retry(outcome);
        }
        let completed = match result {
            Ok(ResultValue::NeedsResolution) => {
                pending.admitted = true;
                if let Some(changes) = pending.fixed.take() {
                    pending.request.params = serde_json::Value::Null;
                    pending.queued = Some(Work::Resolve {
                        changes,
                        response: Some(ResponsePlan::Fixed(JsonRpcResponse::success(
                            serde_json::Value::Null,
                            serde_json::json!({"updated":true}),
                        ))),
                    });
                    self.refresh_command_weight(ticket);
                } else {
                    pending.needs_resolution = true;
                }
                (self.wake)();
                return Ok(true);
            }
            Ok(ResultValue::JoinedAdmission { .. }) => {
                pending.replay = true;
                pending.admitted = true;
                return Ok(true);
            }
            Ok(ResultValue::Reserved(ids)) => {
                let CategoryReservation { stream, name } = pending
                    .category
                    .take()
                    .ok_or("reservation has no resolved category request")?;
                let id = ids
                    .first()
                    .filter(|range| range.kind == "category" && range.end == range.start + 1)
                    .map(|range| u32::try_from(range.start))
                    .transpose()
                    .map_err(|error| error.to_string())?
                    .ok_or("category reservation result missing")?;
                pending.queued = Some(Work::Resolve {
                    changes: vec![StreamCommand {
                        stream: stream.clone(),
                        command: tasty_domain::StructuralCommand::CreateCategory {
                            reserved_id: id,
                            name,
                        },
                    }],
                    response: Some(ResponsePlan::CategoryCreated { stream, id }),
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::Cancelled) => oversized_response(serde_json::Value::Null),
            Ok(ResultValue::Stored(record)) => {
                let bytes = record
                    .response
                    .as_ref()
                    .ok_or("stored structure result has no response")?;
                let mut reply: JsonRpcResponse =
                    serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
                reply.idempotent_replay = true;
                reply
            }
            Ok(ResultValue::Executed(executed)) => {
                let bytes = executed
                    .response
                    .as_ref()
                    .ok_or("committed structure result has no response")?;
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?
            }
            Err(error) => JsonRpcResponse::error(
                serde_json::Value::Null,
                if error.contains("idempotency key") {
                    crate::ipc::protocol::ERR_IDEMPOTENCY_KEY_CONFLICT
                } else if error.contains("capacity exhausted") {
                    crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL
                } else {
                    -32603
                },
                error,
            ),
            other => {
                return Err(format!(
                    "unexpected structural command completion: {other:?}"
                ));
            }
        };
        let pending = self
            .commands
            .pending
            .remove(&ticket)
            .expect("pending command");
        let mut completed = completed;
        completed.id = pending.request.id.unwrap_or_default();
        completed.idempotent_replay |= pending.replay;
        // The stored body is historical. Volatile display work runs only for its first result,
        // after the committed local projection and before the original response is published.
        if completed.error.is_none()
            && !completed.idempotent_replay
            && let Some(display) = pending.display
            && let Some((engine, event)) = display.apply(sessions)
        {
            self.changed_engines.insert(engine);
            if let Some(event) = event {
                self.commands.completed_host_events.push((engine, event));
            }
        }
        self.commands.deliver(pending.reply, completed);
        Ok(true)
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
    pub(crate) fn resolve_headless_requests(
        &mut self,
        session: &EngineSession,
        state: &mut crate::state::RequestContext,
    ) {
        for (ticket, request) in self.requests_needing_resolution() {
            if self.commands.pending.get(&ticket).is_some_and(|pending| matches!(&pending.reply, Reply::Intent { engine, .. } if *engine != session.id)) {
                self.reject_resolved_request(ticket, JsonRpcResponse::invalid_params(serde_json::Value::Null, "intent engine no longer exists"));
                continue;
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
                        crate::core::request_target::unowned_target_message(
                            resource,
                            &request.method,
                        ),
                    ),
                );
                continue;
            }
            self.resolve_ipc_for_engine(ticket, session);
        }
        for (engine, event) in std::mem::take(&mut self.commands.completed_host_events) {
            if engine == session.id
                && let Some(event) = event.resolve(&session.core_state, &state.navigation)
            {
                state.enqueue_host_event(event);
            }
        }
        for result in std::mem::take(&mut self.commands.completed_intents) {
            if let Some(error) = result.response.error {
                tracing::warn!(engine = ?result.engine, origin = ?result.origin, "headless journal intent failed: {}", error.message);
            }
        }
    }
}

#[cfg(feature = "gui")]
impl crate::app::App {
    pub(crate) fn resolve_journal_requests(&mut self) {
        for (ticket, request) in self.journal.requests_needing_resolution() {
            if let Some(id) = self
                .journal
                .commands
                .pending
                .get(&ticket)
                .and_then(|pending| match &pending.reply {
                    Reply::Intent { engine, .. } => Some(*engine),
                    _ => None,
                })
            {
                if let Some(session) = self.engines.session_mut(id) {
                    self.journal.resolve_ipc_for_engine(ticket, session);
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
            self.journal.resolve_ipc_for_engine(ticket, session);
        }
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
        for (engine, event) in std::mem::take(&mut self.journal.commands.completed_host_events) {
            if let Some(context) = self.engines_mut().resolve(engine)
                && let Some(event) = event.resolve(context.engine.core, &context.state.navigation)
            {
                context.state.enqueue_host_event(event);
            }
        }
        for result in std::mem::take(&mut self.journal.commands.completed_intents) {
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

fn retry_outcome(
    result: &Result<ResultValue, String>,
) -> Option<crate::ipc::handler::idempotency::RetryOutcome> {
    use crate::ipc::handler::idempotency::RetryOutcome as O;
    match result {
        Ok(ResultValue::NeedsResolution) => Some(O::Executed),
        Ok(ResultValue::JoinedAdmission { .. }) => Some(O::InFlight),
        Ok(ResultValue::Stored(_)) => Some(O::Replayed),
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
        if !handles(&request.method) {
            return false;
        }
        let Some(process) = manager.and_then(|manager| manager.processes.get(&call.plugin_id))
        else {
            tracing::warn!("structural plugin caller disappeared before admission");
            return true;
        };
        self.admit_request(
            request,
            Reply::Plugin {
                plugin_id: call.plugin_id.clone(),
                call_id: call.call_id,
                binding: process.reply_binding(),
            },
            crate::ipc::handler::idempotency::caller_scope(caller),
            "plugin",
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
        .saturating_add(reply_weight(&pending.reply))
        .saturating_add(
            pending
                .category
                .as_ref()
                .map_or(0, |category| category.name.len() + category.stream.len()),
        )
}

fn reply_weight(_reply: &Reply) -> usize {
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
