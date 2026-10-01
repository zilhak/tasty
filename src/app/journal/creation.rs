//! A live request continuation. The engine owns the unpublished materialization, never this router.
use super::*;
use crate::runtime::effect_runner::{self, ExecutionBinding, Installed};
use crate::runtime::journal_product::{Admission, PreparationInput, ShellRecipe, StreamCommand};
use tasty_core::{
    CreationDestination, CreationPlan, IdKind, OperationId, PreparationResult, StructuralCommand,
    StructuralResult, SurfaceSpec,
};

#[derive(Clone)]
pub(crate) enum ActivationOutcome {
    Ready {activation:Option<u64>,physical:Option<tasty_terminal::ResourceGeneration>},
    Failed(String),
}
pub(crate) type ActivationReceipt=std::sync::Arc<std::sync::OnceLock<ActivationOutcome>>;

pub(super) struct Creation {
    pub(super) ticket: u64,
    binding: crate::runtime::journal_product::EngineBinding,
    input: PreparationInput,
    fixed_plan: Option<CreationPlan>,
    requested_restore:Option<(u32,Option<u64>)>,
    receipt:ActivationReceipt,
    restore_retry:Option<crate::runtime::surface_restorer::RestoreInput>,
    restore_failure:Option<String>,
    stage: Stage,
    public: bool,
    assembly_member:bool,
    published:bool,
    transfers_existing:bool,
    preparation_acked:bool,
    one_shot_input:Option<String>,
    queued: Option<Work>,
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
        started:std::time::Instant,
    },
    Uncertain {reason:String},
    Finish(Installed),
    AwaitPublication(Installed),
    Rejected {
        discard_committed:bool,
        lease: crate::runtime::journal_product::EffectLease,
        reason: String,
        retirement: Option<tasty_terminal::PtyRetirement>,
        answered: bool,
        started:std::time::Instant,
    },
    Failed(String),
    Transition,
}

impl Creation {
    pub(super) fn set_one_shot_input(&mut self,input:Option<String>) {self.one_shot_input=input;}
    pub(super) fn committed(
        ticket: u64,
        binding: crate::runtime::journal_product::EngineBinding,
        worker: &JournalWorker,
        operation: OperationId,
    ) -> Result<Self, String> {
        let mut creation = Self {
            ticket,
            binding,
            public: true,
            assembly_member:false,published:false,transfers_existing:false,preparation_acked:false,one_shot_input:None,
            stage: Stage::Claim,
            fixed_plan: None,requested_restore:None,receipt:Default::default(),restore_retry:None,restore_failure:None,
            queued: None,
            input: PreparationInput {adopt:None,child:None,
                kind: String::new(),
                cwd: None,
                params: serde_json::Value::Null,
                restore: None,
                shell: None,
            },
        };
        creation.submit(
            worker,
            Work::ClaimPreparation {
                stream: creation.binding.stream.clone(),
                operation,
            },
        )?;
        Ok(creation)
    }

    pub(super) fn default_workspace(
        ticket: u64,
        session: &EngineSession,
        worker: &JournalWorker,
    ) -> Result<Self, String> {
        let core = &session.core_state;
        let shell = crate::core::state::ShellConfig::from_settings(&core.runtime.settings);
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
            public: false,
            assembly_member:false,published:false,transfers_existing:false,preparation_acked:false,one_shot_input:None,
            queued: None,
            fixed_plan: None,requested_restore:None,receipt:Default::default(),restore_retry:None,restore_failure:None,
            input: PreparationInput {adopt:None,child:None,
                kind: "terminal".into(),
                cwd: None,
                params: serde_json::json!({}),
                restore: None,
                shell: Some(ShellRecipe {
                    executable: shell.shell,
                    arguments: shell.args,
                    environment: shell.envs,
                    cols: core.runtime.default_cols,
                    rows: core.runtime.default_rows,
                    scrollback_lines: core.runtime.settings.general.scrollback_lines,
                    disk_scrollback: core.runtime.settings.performance.scrollback_disk_swap,
                    startup_command: core.runtime.settings.general.startup_command.clone(),
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
        let admission=Work::Admit(Admission {
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
                });
        Ok(Self {
            ticket,
            binding,
            restore_retry:Some(request.clone()),restore_failure:None,
            input: request.input,
            requested_restore:match &request.plan.destination {CreationDestination::Restore {surface,previous_activation}=>Some((*surface,*previous_activation)),_=>None},
            fixed_plan: Some(request.plan),receipt:Default::default(),
            stage: Stage::Admit,
            public: false,
            assembly_member:false,published:false,transfers_existing:false,preparation_acked:false,one_shot_input:None,
            queued: Some(admission),
        })
    }

    fn submit(&mut self, _worker: &JournalWorker, work: Work) -> Result<(), String> {
        if self.queued.is_some() {
            return Err("materialization already has a pending worker step".into());
        }
        self.queued = Some(work);
        Ok(())
    }

    fn flush(&mut self, worker: &JournalWorker) -> Result<bool, String> {
        let Some(work) = self.queued.take() else {
            return Ok(true);
        };
        match worker.submit_owned(Request {
            ticket: self.ticket,
            work,
        }) {
            Ok(()) => Ok(true),
            Err((crate::runtime::journal_product::SubmitError::Busy, request)) => {
                self.queued = Some(request.work);
                Ok(false)
            }
            Err((error, _)) => Err(format!("materialization submission: {error:?}")),
        }
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
                            attach_mapping: None,
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
                    Work::Resolve {
                        changes: vec![StreamCommand {
                            stream: self.binding.stream.clone(),
                            command: StructuralCommand::PrepareCreation {
                                operation: OperationId(String::new()),
                                command_id: String::new(),
                                input,
                                plan,
                            },
                        }],
                        response: None,
                    },
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
                self.requested_restore=match &claimed.plan.destination {CreationDestination::Restore {surface,previous_activation}=>Some((*surface,*previous_activation)),_=>None};
                self.transfers_existing=claimed.input.adopt.is_some();
                self.assembly_member=matches!(claimed.plan.destination,CreationDestination::Assembly {..});
                let lease = claimed.lease.clone();
                let binding = ExecutionBinding {
                    stream: self.binding.stream.clone(),
                    runtime_epoch: self.binding.runtime_epoch,
                    engine_incarnation: self.binding.incarnation,
                };
                match effect_runner::prepare(&mut session.borrow_mut(), &binding, claimed) {
                    Ok(mut prepared) => {
                        prepared.set_one_shot_input(self.one_shot_input.take());
                        let preparation_result=if prepared.is_deferred() {PreparationResult::Deferred {data:None}}else {PreparationResult::Ready {data:None}};
                        let operation = prepared.lease.operation.clone();
                        session
                            .pending_materializations
                            .insert(operation.clone(), prepared);
                        self.submit(
                            worker,
                            Work::Prepared {
                                lease,
                                result: preparation_result,
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
            (Stage::Prepared(operation),ResultValue::Executed(_))=>{self.preparation_acked=true;Stage::Prepared(operation)},
            (Stage::Installing { installed,started,.. }, ResultValue::Executed(_)) => Stage::Installing {
                installed,
                answered: true,
                started,
            },
            (Stage::Uncertain {reason},ResultValue::Executed(_))=>return Err(format!("resource publication is uncertain: {reason}")),
            (Stage::Finish(installed), ResultValue::Executed(_)) if self.assembly_member && !self.published => Stage::AwaitPublication(installed),
            (Stage::Finish(_), ResultValue::Executed(_)) => return Ok(true),
            (
                Stage::Rejected {
                    lease,
                    reason,
                    retirement,
                    started,
                    discard_committed,
                    ..
                },
                ResultValue::Executed(_),
            ) => Stage::Rejected {
                discard_committed,
                lease,
                reason,
                retirement,
                answered: true,
                started,
            },
            (Stage::Failed(reason), _) if self.public => {let _=self.receipt.set(ActivationOutcome::Failed(reason));return Ok(true);}
            (Stage::Claim, ResultValue::Executed(_)) if self.public => return Ok(true),
            (Stage::Failed(reason), _) if self.restore_retry.is_some()=> {self.restore_failure=Some(reason.clone());let _=self.receipt.set(ActivationOutcome::Failed(reason));return Ok(true);},
            (Stage::Failed(reason), _) => return Err(reason),
            _ => return Err("materialization completion arrived outside its request phase".into()),
        };
        Ok(false)
    }

    pub(super) fn authorize_installation(
        &mut self,
        session: &mut EngineSession,
        events: &[tasty_core::RecordedEvent],
    ) -> Result<Option<effect_runner::Installation>, String> {
        let Stage::Prepared(operation) = &self.stage else {
            return Ok(None);
        };
        if events.iter().any(|recorded|matches!(&recorded.event,tasty_core::DomainEvent::OperationAwaitingCleanup {id,cleanup:tasty_core::CleanupPlan::DiscardPrepared {..},..} if id==operation)) {
            let candidate=session.pending_materializations.remove(operation).ok_or("discarded assembly member lost its private owner")?;
            let lease=candidate.lease.clone();let retirement=match self.discard_candidate(session,candidate) {Some(receipt)=>receipt,None=>return Ok(None)};
            self.stage=Stage::Rejected {discard_committed:true,lease,reason:"assembly cancelled before installation".into(),retirement,answered:self.preparation_acked,started:std::time::Instant::now()};
            return Ok(None);
        }
        if !events.iter().any(|recorded|matches!(&recorded.event,
            tasty_core::DomainEvent::OperationAwaitingCleanup { id, cleanup:tasty_core::CleanupPlan::InstallPrepared { .. }, .. } if id==operation)) { return Ok(None); }
        let operation = operation.clone();
        let result = session
            .pending_materializations
            .get_mut(&operation)
            .ok_or("prepared operation lost its resource owner")?
            .begin_installation(&session.runtime.surface_registry);
        match result {
            Ok(installation) => Ok(Some(installation)),
            Err(error) => {
                let candidate = session
                    .pending_materializations
                    .remove(&operation)
                    .expect("checked candidate");
                let lease = candidate.lease.clone();
                let retirement=match self.discard_candidate(session,candidate) {Some(receipt)=>receipt,None=>return Ok(None)};
                self.stage = Stage::Rejected {
                    discard_committed:false,
                    lease,
                    reason: error.to_string(),
                    retirement,
                    answered: self.preparation_acked,
                    started:std::time::Instant::now(),
                };
                Ok(None)
            }
        }
    }

    fn discard_candidate(&mut self,session:&mut EngineSession,candidate:effect_runner::PreparedMaterialization)->Option<Option<tasty_terminal::PtyRetirement>> {
        match candidate.discard(&mut session.borrow_mut()) {
            Ok(receipt)=>Some(receipt),
            Err((candidate,reason))=> {
                let lease=candidate.lease.clone();session.pending_materializations.insert(lease.operation.clone(),candidate);
                self.queued=Some(Work::PreparationUncertain {lease,reason:reason.clone()});self.stage=Stage::Uncertain {reason};None
            },
        }
    }

    pub(super) fn leaf_for_publication(
        &mut self,
        session: &mut EngineSession,
        events: &[tasty_core::RecordedEvent],
    ) -> Result<Option<effect_runner::PreparedLeaf>, String> {
        let installed=match &self.stage {Stage::Finish(installed)|Stage::AwaitPublication(installed)=>installed,_=>return Ok(None)};
        let surface=session.pending_materializations.get(&installed.lease.operation).and_then(|prepared|prepared.surface_id()).ok_or("installed leaf has no original candidate")?;
        if !events.iter().any(|recorded|matches!(&recorded.event,
            tasty_core::DomainEvent::SurfaceActivationChanged {id,activation,..} if *id==surface && activation.generation==installed.lease.resource_generation)) { return Ok(None); }
        self.published=true;
        let prepared = session
            .pending_materializations
            .remove(&installed.lease.operation)
            .ok_or("installed operation lost its kind owner")?;
        prepared
            .into_leaf(installed)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub(super) fn failed_restore(&mut self,session:&mut EngineSession)->Option<crate::runtime::surface_restorer::RestoreInput> {
        let reason=self.restore_failure.take()?;
        let request=self.restore_retry.take()?;
        let placeholder=session.runtime.surfaces.get_mut(&request.surface_id)?.as_any_mut().downcast_mut::<crate::runtime::surface_restorer::JournalPlaceholder>()?;
        placeholder.attempts=placeholder.attempts.saturating_add(1);placeholder.failure=Some(reason);
        // Retain the old content/seed, but stop automatic retries at the legacy terminal cap.
        (placeholder.attempts<crate::runtime::surface_restorer::MAX_ACTIVATION_ATTEMPTS).then_some(request)
    }
    pub(super) fn join_restore(&self,surface:u32,activation:Option<u64>)->Option<ActivationReceipt> {
        (self.requested_restore==Some((surface,activation))).then(||self.receipt.clone())
    }
    pub(super) fn acknowledge_publication(&self,session:&EngineSession) {
        if !self.published {return;}
        let Some((surface,_))=self.requested_restore else{return;};
        let activation=session.core_state.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
        let physical=session.runtime.terminals.generation(surface);
        let _=self.receipt.set(ActivationOutcome::Ready {activation,physical});
    }
    pub(super) fn publication_released(&self)->bool {self.published && matches!(self.stage,Stage::AwaitPublication(_))}

    pub(super) fn installed(&mut self, installed: Installed) {
        self.stage = Stage::Installing {
            installed,
            answered: self.preparation_acked,
            started:std::time::Instant::now(),
        };
    }

    #[cfg(feature = "gui")]
    pub(super) fn input_generation(
        &self,
        surface: u32,
    ) -> Option<Option<tasty_terminal::ResourceGeneration>> {
        match &self.stage {
            Stage::Installing { installed, .. } | Stage::Finish(installed) | Stage::AwaitPublication(installed)
                if installed.surface_id == surface =>
            {
                Some(installed.previous_resource)
            }
            _ => None,
        }
    }

    pub(super) fn pauses_observation(&self) -> bool {
        (self.transfers_existing && matches!(self.stage,Stage::Prepared(_)|Stage::Rejected {..})) || matches!(self.stage, Stage::Installing { .. } | Stage::Finish(_) | Stage::AwaitPublication(_) | Stage::Uncertain {..})
    }

    pub(super) fn needs_cleanup_poll(&self) -> bool {
        matches!(
            self.stage,
            Stage::Installing { answered: true, .. } | Stage::Rejected { answered: true, .. }
        )
    }

    pub(super) fn poll_cleanup(
        &mut self,
        worker: &JournalWorker,
        view:crate::runtime::journal_product::CompletionView,
        session:&mut EngineSession,
    ) -> Result<(), String> {
        if !self.flush(worker)? {
            return Ok(());
        }
        if let Stage::Rejected {
            lease,
            reason,
            retirement,
            answered: true,
            started,
            discard_committed,
        } = &self.stage
        {
            let phase=retirement.as_ref().map(|receipt|receipt.observation().phase);
            let complete=matches!(phase,None|Some(tasty_terminal::PtyPhase::Reaped));
            if !complete && (phase==Some(tasty_terminal::PtyPhase::WaitFailed) || started.elapsed()>=std::time::Duration::from_secs(5)) {
                let reason="private preparation owner could not confirm reap before its deadline".to_owned();
                self.submit(worker,Work::PreparationUncertain {lease:lease.clone(),reason:reason.clone()})?;
                self.stage=Stage::Uncertain {reason};
                self.flush(worker)?;
                return Ok(());
            }
            if complete {
                let reason = reason.clone();
                self.submit(
                    worker,
                    if *discard_committed {Work::CleanupFinished {lease:lease.clone(),view}}else {Work::InstallationRejected {
                        lease: lease.clone(),
                        reason: reason.clone(),
                    }},
                )?;
                self.stage = Stage::Failed(reason);
                self.flush(worker)?;
            }
            return Ok(());
        }
        let ready=if let Stage::Installing {installed,answered:true,started}=&mut self.stage {
            if let Err(error)=installed.poll_input(&mut session.borrow_mut()) {
                let reason=error.to_string();let lease=installed.lease.clone();
                self.submit(worker,Work::PreparationUncertain {lease,reason:reason.clone()})?;
                self.stage=Stage::Uncertain {reason};self.flush(worker)?;return Ok(());
            }
            if installed.waiting_input() {return Ok(());}
            let uncertain=match installed.cleanup_complete() {
                Ok(true)=>None,
                Ok(false) if started.elapsed()<std::time::Duration::from_secs(5)=>return Ok(()),
                Ok(false)=>Some("old resource cleanup did not complete before its deadline".to_owned()),
                Err(error)=>Some(error.to_string()),
            };
            if let Some(reason)=uncertain {
                self.submit(worker,Work::PreparationUncertain {lease:installed.lease.clone(),reason:reason.clone()})?;
                self.stage=Stage::Uncertain {reason};
                self.flush(worker)?;
                return Ok(());
            }
            true
        } else {false};
        if ready && let Stage::Installing {installed,..}=&self.stage {
            self.submit(
                worker,
                Work::CleanupFinished {
                    lease: installed.lease.clone(),
                    view,
                },
            )?;
            let Stage::Installing { installed, .. } =
                std::mem::replace(&mut self.stage, Stage::Transition)
            else {
                unreachable!("checked installation phase")
            };
            self.stage = Stage::Finish(installed);
            self.flush(worker)?;
        }
        Ok(())
    }
}

impl Drop for Creation {
    fn drop(&mut self) {
        let reason=match &self.stage {Stage::Failed(reason)|Stage::Uncertain {reason}=>reason.clone(),_=>"activation owner ended without a published resource receipt".into()};
        let _=self.receipt.set(ActivationOutcome::Failed(reason));
    }
}
