//! Worker replies settle the accepted command before publishing its original response.
use super::*;

impl JournalApplication {
    pub(in crate::app::journal) fn answer_command(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, JournalError>,
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
                    let mut creation = crate::app::journal::creation::Creation::committed(
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
            Err(error) => {
                JsonRpcResponse::error(serde_json::Value::Null, error.ipc_code(), error.to_string())
            }
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
            #[cfg(feature = "gui")]
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
}
