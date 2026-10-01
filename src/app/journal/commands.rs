//! Accepted structural requests retain their original identity until a committed wire reply exists.
mod assembly;
mod category;
mod child;
mod close;
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
    pub(crate) view: std::sync::Weak<()>,
    pub(crate) selection: std::sync::Weak<()>,
    pub(crate) activate_surface: Option<u32>,
    pub(crate) close_empty_engine: bool,
    pub(crate) after_create: Option<crate::intent::CreateFollowup>,
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
                pending.needs_resolution && pending.request.method == "workspace.create"
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
            Ok(ResultValue::Reserved(ids)) if pending.resource.is_some() => {
                pending.queued = Some(
                    pending
                        .resource
                        .as_mut()
                        .expect("resource reservation")
                        .reserved(ids)?,
                );
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::ClosedCaptured { input, undo }) => {
                let closing = pending
                    .closing
                    .as_mut()
                    .ok_or("close capture lost its request")?;
                closing.undo = undo.clone();
                closing.input_ref = Some(*input);
                pending.queued = Some(if let Some(replacement) = pending.resource.as_mut() {
                    replacement.reservation()?
                } else {
                    closing.stored(*input)
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            #[cfg(feature = "gui")]
            Ok(ResultValue::InputStored(input)) if pending.forward.is_some() => {
                let draft = pending.forward.as_ref().expect("forward input owner");
                pending.queued = Some(Work::Resolve {
                    changes: vec![StreamCommand {
                        stream: draft.stream.clone(),
                        command: tasty_core::StructuralCommand::PrepareForward {
                            operation: tasty_core::OperationId(String::new()),
                            command_id: String::new(),
                            input: *input,
                        },
                    }],
                    response: Some(ResponsePlan::Fixed(draft.response.clone())),
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::InputStored(input)) if pending.replacing.is_some() => {
                pending.queued = Some(
                    pending
                        .replacing
                        .as_ref()
                        .expect("replacement input owner")
                        .stored(*input),
                );
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::InputStored(input)) if pending.closing.is_some() => {
                let closing = pending.closing.as_mut().expect("close input owner");
                pending.queued = Some(if let Some(close_input) = closing.input_ref {
                    let Work::Resolve {
                        mut changes,
                        response,
                    } = closing.stored(close_input)
                    else {
                        unreachable!("close resolution")
                    };
                    let Work::Resolve {
                        changes: replacement,
                        ..
                    } = pending
                        .resource
                        .as_ref()
                        .ok_or("replacement draft disappeared")?
                        .stored(*input)
                    else {
                        unreachable!("creation resolution")
                    };
                    changes.extend(replacement);
                    Work::Resolve { changes, response }
                } else {
                    closing.input_ref = Some(*input);
                    if let Some(replacement) = pending.resource.as_mut() {
                        replacement.reservation()?
                    } else {
                        closing.stored(*input)
                    }
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::AssemblyResolved {
                stream,
                input,
                plan,
            }) => {
                pending.queued = Some(match (input, plan) {
                    (Some(input), Some(plan)) => Work::Resolve {
                        changes: vec![StreamCommand {
                            stream: stream.clone(),
                            command: tasty_core::StructuralCommand::PrepareAssembly {
                                operation: tasty_core::OperationId(String::new()),
                                command_id: String::new(),
                                input: *input,
                                plan: plan.clone(),
                            },
                        }],
                        response: Some(if pending.request.method == "preset.apply" {
                            let root = plan.snapshot.root;
                            let mut response =
                                serde_json::json!({"applied":true,"kind":root.kind.label()});
                            response[format!("{}_id", root.kind.label())] =
                                serde_json::json!(root.id);
                            ResponsePlan::Fixed(JsonRpcResponse::success(
                                serde_json::Value::Null,
                                response,
                            ))
                        } else {
                            ResponsePlan::AssemblyRestored {
                                stream: stream.clone(),
                                root: plan.snapshot.root,
                                surfaces: plan.snapshot.surfaces.keys().copied().collect(),
                                presentation: plan.snapshot.presentation.clone(),
                            }
                        }),
                    },
                    (None, None) => Work::Resolve {
                        changes: Vec::new(),
                        response: Some(ResponsePlan::Fixed(JsonRpcResponse::success(
                            serde_json::Value::Null,
                            serde_json::json!({"restored":false}),
                        ))),
                    },
                    _ => return Err("undo preparation result is incomplete".into()),
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::InputStored(input)) if pending.resource.is_some() => {
                pending.queued = Some(
                    pending
                        .resource
                        .as_ref()
                        .expect("resource input")
                        .stored(*input),
                );
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::Executed(executed))
                if executed.status == tasty_event_store::CommandStatus::InProgress =>
            {
                pending.waiting_command = Some(executed.command_id.clone());
                #[cfg(feature = "gui")]
                let mut forward_start = None;
                #[cfg(feature = "gui")]
                if !pending.replay
                    && let Some(draft) = pending.forward.take()
                {
                    let progress: crate::runtime::journal_product::ResponseProgress =
                        serde_json::from_slice(
                            executed
                                .response
                                .as_deref()
                                .ok_or("forward command progress missing")?,
                        )
                        .map_err(|error| error.to_string())?;
                    let Some(tasty_core::StructuralResult::Pending { operation }) =
                        progress.results.first()
                    else {
                        return Err("forward command operation missing".into());
                    };
                    forward_start = Some((draft, operation.clone()));
                }

                if !pending.replay
                    && let Some(request) = pending.resource.take()
                {
                    let progress: crate::runtime::journal_product::ResponseProgress =
                        serde_json::from_slice(
                            executed
                                .response
                                .as_deref()
                                .ok_or("public creation progress missing")?,
                        )
                        .map_err(|error| error.to_string())?;
                    let Some(tasty_core::StructuralResult::Pending { operation }) =
                        progress.results.last()
                    else {
                        return Err("public creation operation missing".into());
                    };
                    pending.one_shot_reserved =
                        request.one_shot_input.as_ref().map_or(0, String::len);
                    pending.created = Some(create::Completed::from_request(&request));
                    let creation_ticket = self.next_ticket;
                    self.next_ticket += 1;
                    let mut creation = super::creation::Creation::committed(
                        creation_ticket,
                        request.binding,
                        &self.worker,
                        operation.clone(),
                    )?;
                    creation.set_one_shot_input(request.one_shot_input);
                    if self
                        .creations
                        .insert((request.engine, creation.ticket), creation)
                        .is_some()
                    {
                        return Err("engine already owns a materialization continuation".into());
                    }
                }
                self.refresh_command_weight(ticket);
                #[cfg(feature = "gui")]
                if let Some((draft, operation)) = forward_start {
                    self.start_forward(draft, operation)?;
                }
                return Ok(true);
            }
            Ok(ResultValue::Stored(record) | ResultValue::Command(record))
                if record.status == tasty_event_store::CommandStatus::InProgress =>
            {
                pending.replay |= matches!(result, Ok(ResultValue::Stored(_)));
                pending.waiting_command = Some(record.command_id.clone());
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
                        command: tasty_core::StructuralCommand::CreateCategory {
                            reserved_id: id,
                            name,
                        },
                    }],
                    response: Some(ResponsePlan::CategoryCreated { stream, id }),
                });
                self.refresh_command_weight(ticket);
                return Ok(true);
            }
            Ok(ResultValue::RecoveryRequired {
                command_id,
                reason,
                replay,
            }) => {
                let mut response = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED,
                    format!("command {command_id} requires recovery: {reason}"),
                );
                response.idempotent_replay = *replay;
                response
            }
            Ok(ResultValue::Cancelled) => oversized_response(serde_json::Value::Null),
            Ok(ResultValue::Stored(record) | ResultValue::Command(record)) => {
                let bytes = record
                    .response
                    .as_ref()
                    .ok_or("stored structure result has no response")?;
                let mut reply: JsonRpcResponse =
                    serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
                reply.idempotent_replay = matches!(result, Ok(ResultValue::Stored(_)));
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
        let mut pending = self
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
        if completed.error.is_none()
            && !completed.idempotent_replay
            && let Some(created) = pending.created
        {
            if let Reply::Intent {
                view: Some(view),
                origin,
                ..
            } = &mut pending.reply
                && origin.is_user()
            {
                view.tutorial = created.tutorial.clone();
                #[cfg(feature = "gui")]
                if view.tutorial_preparation.is_some() {
                    view.tutorial_surface = sessions
                        .iter()
                        .find(|session| session.id == created.engine)
                        .and_then(|session| {
                            crate::app::engine_action::SurfaceBinding::capture(
                                &session.as_ref().read(),
                                created.surface,
                            )
                            .map(|target| (created.surface, target))
                        });
                }
            }
            if created.activate
                && let Reply::Intent {
                    view: Some(view),
                    origin,
                    ..
                } = &mut pending.reply
                && origin.is_user()
            {
                view.activate_surface = Some(created.surface);
            }
            if matches!(
                created.destination,
                tasty_core::CreationDestination::Restore { .. }
            ) {
                if let Some(items) = self.restoration_ready.get_mut(&created.engine) {
                    items.retain(|item| item.surface_id != created.surface);
                }
            }
            created.notify(sessions, &mut self.commands.completed_host_events);
        }

        if completed.error.is_none()
            && let Reply::Resume(resume) = &mut pending.reply
        {
            if resume.generation.is_none() {
                resume.generation = sessions
                    .iter()
                    .find(|session| session.id == resume.engine)
                    .and_then(|session| session.runtime.terminals.generation(resume.surface));
            }
        }
        match pending.reply {
            Reply::Resume(mut resume) => {
                if let Some((surface, activation)) = resume.advance(&mut completed) {
                    self.continue_live_resume(resume, surface, activation);
                } else {
                    self.commands.deliver(Reply::Resume(resume), completed);
                }
            }
            reply => self.commands.deliver(reply, completed),
        }
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
        session: &mut EngineSession,
        state: &mut crate::state::RequestContext,
        services: &crate::app::services::AppServices,
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
            if request.method == "remote.structural" {
                self.resolve_remote_request(ticket, session, services);
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
