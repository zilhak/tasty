//! A live request continuation. The engine owns the unpublished materialization, never this router.
use super::*;
use crate::runtime::effect_runner::{self, ExecutionBinding, Installed};
use crate::runtime::journal_product::{Admission, PreparationInput, ShellRecipe, StreamCommand};
use tasty_domain::{
    CreationDestination, CreationPlan, IdKind, OperationId, PreparationResult, StructuralCommand,
    StructuralResult, SurfaceSpec,
};

pub(super) struct Creation {
    pub(super) ticket: u64,
    binding: crate::runtime::journal_product::EngineBinding,
    input: PreparationInput,
    fixed_plan: Option<CreationPlan>,
    stage: Stage,
}

enum Stage {
    Admit,
    Reserve,
    Input(Vec<tasty_event_store::IdRange>),
    Commit,
    Claim,
    Prepared(OperationId),
    Installing {
        installed: Installed,
        answered: bool,
    },
    Finish(Installed),
    Rejected {
        lease: crate::runtime::journal_product::EffectLease,
        reason: String,
        retirement: Option<tasty_terminal::PtyRetirement>,
        answered: bool,
    },
    Failed(String),
    Transition,
}

impl Creation {
    pub(super) fn default_workspace(
        ticket: u64,
        session: &EngineSession,
        worker: &JournalWorker,
    ) -> Result<Self, String> {
        let core = &session.core_state;
        let shell = crate::core::state::ShellConfig::from_settings(&core.settings);
        let binding = session
            .journal_binding
            .clone()
            .ok_or("default workspace has no engine binding")?;
        worker
            .submit(Request {
                ticket,
                work: Work::Admit(Admission {
                    key: None,
                    original_digest: b"bootstrap.default_workspace".to_vec(),
                    actor: "system".into(),
                    origin: "bootstrap".into(),
                    causation_id: None,
                }),
            })
            .map_err(|error| format!("default workspace admission: {error:?}"))?;
        Ok(Self {
            ticket,
            binding,
            stage: Stage::Admit,
            fixed_plan: None,
            input: PreparationInput {
                kind: "terminal".into(),
                cwd: None,
                params: serde_json::json!({}),
                restore: None,
                shell: Some(ShellRecipe {
                    executable: shell.shell,
                    arguments: shell.args,
                    environment: shell.envs,
                    cols: core.default_cols,
                    rows: core.default_rows,
                    scrollback_lines: core.settings.general.scrollback_lines,
                    disk_scrollback: core.settings.performance.scrollback_disk_swap,
                    startup_command: core.settings.general.startup_command.clone(),
                    restore_command: None,
                }),
            },
        })
    }

    pub(super) fn restore(
        ticket: u64,
        session: &EngineSession,
        worker: &JournalWorker,
        request: crate::runtime::surface_restorer::RestoreInput,
    ) -> Result<Self, String> {
        let binding = session
            .journal_binding
            .clone()
            .ok_or("restoring engine has no binding")?;
        worker
            .submit(Request {
                ticket,
                work: Work::Admit(Admission {
                    key: None,
                    original_digest: serde_json::to_vec(&(
                        "restore",
                        &binding.stream,
                        request.surface_id,
                        &request.plan,
                    ))
                    .map_err(|error| error.to_string())?,
                    actor: "system".into(),
                    origin: "selected-restore".into(),
                    causation_id: None,
                }),
            })
            .map_err(|error| format!("restore admission: {error:?}"))?;
        Ok(Self {
            ticket,
            binding,
            input: request.input,
            fixed_plan: Some(request.plan),
            stage: Stage::Admit,
        })
    }

    fn submit(&self, worker: &JournalWorker, work: Work) -> Result<(), String> {
        worker
            .submit(Request {
                ticket: self.ticket,
                work,
            })
            .map_err(|error| format!("materialization submission: {error:?}"))
    }

    pub(super) fn answered(
        &mut self,
        worker: &JournalWorker,
        session: &mut EngineSession,
        value: ResultValue,
    ) -> Result<bool, String> {
        let stage = std::mem::replace(&mut self.stage, Stage::Transition);
        self.stage = match (stage, value) {
            (Stage::Admit, ResultValue::NeedsResolution) if self.fixed_plan.is_some() => {
                self.submit(worker, Work::PutPreparation(self.input.clone()))?;
                Stage::Input(Vec::new())
            }
            (Stage::Admit, ResultValue::NeedsResolution) => {
                self.submit(
                    worker,
                    Work::Reserve(vec![
                        (IdKind::Workspace, 1),
                        (IdKind::Pane, 1),
                        (IdKind::Tab, 1),
                        (IdKind::Surface, 1),
                    ]),
                )?;
                Stage::Reserve
            }
            (Stage::Reserve, ResultValue::Reserved(ids)) => {
                self.submit(worker, Work::PutPreparation(self.input.clone()))?;
                Stage::Input(ids)
            }
            (Stage::Input(ids), ResultValue::InputStored(input)) => {
                let id = |kind: IdKind| -> Result<u32, String> {
                    let range = ids
                        .iter()
                        .find(|range| range.kind == kind.label())
                        .ok_or("missing reserved identity")?;
                    u32::try_from(range.start).map_err(|error| error.to_string())
                };
                let plan = if let Some(plan) = &self.fixed_plan {
                    plan.clone()
                } else {
                    CreationPlan {
                        destination: CreationDestination::Workspace {
                            workspace: id(IdKind::Workspace)?,
                            pane: id(IdKind::Pane)?,
                            tab: id(IdKind::Tab)?,
                            name: "Workspace 1".into(),
                            category: 0,
                            subtitle: String::new(),
                            description: String::new(),
                        },
                        surface: SurfaceSpec {
                            id: id(IdKind::Surface)?,
                            kind: "terminal".into(),
                            data: None,
                        },
                        tab_name: "Shell".into(),
                        explicit_name: None,
                    }
                };
                self.submit(
                    worker,
                    Work::Resolve(vec![StreamCommand {
                        stream: self.binding.stream.clone(),
                        command: StructuralCommand::PrepareCreation {
                            operation: OperationId(String::new()),
                            command_id: String::new(),
                            input,
                            plan,
                        },
                    }]),
                )?;
                Stage::Commit
            }
            (Stage::Commit, ResultValue::Executed(executed)) => {
                let progress: Vec<StructuralResult> = serde_json::from_slice(
                    executed
                        .response
                        .as_deref()
                        .ok_or("creation progress missing")?,
                )
                .map_err(|error| error.to_string())?;
                let [StructuralResult::Pending { operation }] = &progress[..] else {
                    return Err("creation returned no pending operation".into());
                };
                self.submit(
                    worker,
                    Work::ClaimPreparation {
                        stream: self.binding.stream.clone(),
                        operation: operation.clone(),
                    },
                )?;
                Stage::Claim
            }
            (Stage::Claim, ResultValue::Claimed(claimed)) => {
                let lease = claimed.lease.clone();
                let binding = ExecutionBinding {
                    stream: self.binding.stream.clone(),
                    runtime_epoch: self.binding.runtime_epoch,
                    engine_incarnation: self.binding.incarnation,
                };
                match effect_runner::prepare(&mut session.borrow_mut(), &binding, claimed) {
                    Ok(prepared) => {
                        let operation = prepared.lease.operation.clone();
                        session
                            .pending_materializations
                            .insert(operation.clone(), prepared);
                        self.submit(
                            worker,
                            Work::Prepared {
                                lease,
                                result: PreparationResult::Ready { data: None },
                            },
                        )?;
                        Stage::Prepared(operation)
                    }
                    Err(error) => {
                        let reason = error.to_string();
                        self.submit(
                            worker,
                            Work::Prepared {
                                lease,
                                result: PreparationResult::Failed {
                                    reason: reason.clone(),
                                },
                            },
                        )?;
                        Stage::Failed(reason)
                    }
                }
            }
            (Stage::Installing { installed, .. }, ResultValue::Executed(_)) => Stage::Installing {
                installed,
                answered: true,
            },
            (Stage::Finish(_), ResultValue::Executed(_)) => return Ok(true),
            (
                Stage::Rejected {
                    lease,
                    reason,
                    retirement,
                    ..
                },
                ResultValue::Executed(_),
            ) => Stage::Rejected {
                lease,
                reason,
                retirement,
                answered: true,
            },
            (Stage::Failed(reason), _) => return Err(reason),
            _ => return Err("materialization completion arrived outside its request phase".into()),
        };
        Ok(false)
    }

    pub(super) fn authorize_installation(
        &mut self,
        session: &mut EngineSession,
        events: &[tasty_domain::RecordedEvent],
    ) -> Result<Option<effect_runner::Installation>, String> {
        let Stage::Prepared(operation) = &self.stage else {
            return Ok(None);
        };
        if !events.iter().any(|recorded|matches!(&recorded.event,
            tasty_domain::DomainEvent::OperationAwaitingCleanup { id, cleanup:tasty_domain::CleanupPlan::InstallPrepared { .. }, .. } if id==operation)) { return Ok(None); }
        let operation = operation.clone();
        let result = session
            .pending_materializations
            .get_mut(&operation)
            .ok_or("prepared operation lost its resource owner")?
            .begin_installation(&session.core_state);
        match result {
            Ok(installation) => Ok(Some(installation)),
            Err(error) => {
                let candidate = session
                    .pending_materializations
                    .remove(&operation)
                    .expect("checked candidate");
                let lease = candidate.lease.clone();
                let retirement = candidate.discard();
                self.stage = Stage::Rejected {
                    lease,
                    reason: error.to_string(),
                    retirement,
                    answered: false,
                };
                Ok(None)
            }
        }
    }

    pub(super) fn leaf_for_publication(
        &mut self,
        session: &mut EngineSession,
        events: &[tasty_domain::RecordedEvent],
    ) -> Result<Option<live_projection::PreparedLeaf>, String> {
        let Stage::Finish(installed) = &self.stage else {
            return Ok(None);
        };
        if !events.iter().any(|recorded|matches!(&recorded.event,
            tasty_domain::DomainEvent::OperationFinished { id, outcome:tasty_domain::OperationOutcome::Succeeded } if *id==installed.lease.operation)) { return Ok(None); }
        let prepared = session
            .pending_materializations
            .remove(&installed.lease.operation)
            .ok_or("installed operation lost its kind owner")?;
        prepared
            .into_leaf(installed)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub(super) fn installed(&mut self, installed: Installed) {
        self.stage = Stage::Installing {
            installed,
            answered: false,
        };
    }

    pub(super) fn poll_cleanup(&mut self, worker: &JournalWorker) -> Result<(), String> {
        if let Stage::Rejected {
            lease,
            reason,
            retirement,
            answered: true,
        } = &self.stage
        {
            let complete = match retirement
                .as_ref()
                .map(|receipt| receipt.observation().phase)
            {
                None | Some(tasty_terminal::PtyPhase::Reaped) => true,
                Some(tasty_terminal::PtyPhase::WaitFailed) => {
                    return Err("private preparation owner could not confirm reap".into());
                }
                _ => false,
            };
            if complete {
                let reason = reason.clone();
                self.submit(
                    worker,
                    Work::InstallationRejected {
                        lease: lease.clone(),
                        reason: reason.clone(),
                    },
                )?;
                self.stage = Stage::Failed(reason);
            }
            return Ok(());
        }
        if let Stage::Installing {
            installed,
            answered: true,
        } = &self.stage
            && installed
                .cleanup_complete()
                .map_err(|error| error.to_string())?
        {
            self.submit(
                worker,
                Work::CleanupFinished {
                    lease: installed.lease.clone(),
                },
            )?;
            let Stage::Installing { installed, .. } =
                std::mem::replace(&mut self.stage, Stage::Transition)
            else {
                unreachable!("checked installation phase")
            };
            self.stage = Stage::Finish(installed);
        }
        Ok(())
    }
}
