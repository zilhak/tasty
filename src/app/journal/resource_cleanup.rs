//! Application scheduling of committed close obligations. The resources stay on EngineSession.
use super::*;
use tasty_core::{OperationId, OperationOutcome};
#[derive(Clone, Copy)]
enum Phase {
    Claim,
    Running,
    Reconcile,
    Reconciled,
    Finish { uncertain: bool },
}
pub(super) struct Cleanup {
    pub(super) engine: EngineId,
    operation: OperationId,
    phase: Phase,
    queued: Option<Work>,
    next_reconcile: std::time::Instant,
}
impl JournalApplication {
    pub(super) fn cleanup_pauses_observation(&self) -> bool {
        self.resource_cleanups
            .values()
            .any(|entry| !matches!(entry.phase, Phase::Reconcile | Phase::Reconciled))
    }
    pub(super) fn resource_cleanup_deadline(&self) -> Option<std::time::Instant> {
        self.resource_cleanups
            .values()
            .map(|entry| {
                if matches!(entry.phase, Phase::Reconcile) {
                    entry.next_reconcile
                } else {
                    std::time::Instant::now() + std::time::Duration::from_millis(10)
                }
            })
            .min()
    }

    pub(crate) fn has_resource_cleanup(&self, engine: EngineId) -> bool {
        self.resource_cleanups
            .values()
            .any(|cleanup| cleanup.engine == engine)
    }

    pub(super) fn queue_resource_retirement(
        &mut self,
        engine: EngineId,
        stream: String,
        operation: OperationId,
    ) -> Result<(), String> {
        if self
            .resource_cleanups
            .values()
            .any(|entry| entry.engine == engine && entry.operation == operation)
        {
            return Err("duplicate retirement publication".into());
        }
        let ticket = self.next_ticket;
        self.next_ticket = ticket
            .checked_add(1)
            .ok_or("journal ticket range exhausted")?;
        self.resource_cleanups.insert(
            ticket,
            Cleanup {
                engine,
                operation: operation.clone(),
                phase: Phase::Claim,
                next_reconcile: std::time::Instant::now(),
                queued: Some(Work::ClaimRetirement { stream, operation }),
            },
        );
        (self.wake)();
        Ok(())
    }
    pub(super) fn poll_resource_cleanup(
        &mut self,
        sessions: &mut [&mut EngineSession],
        mut plugins: Option<&mut crate::plugin::PluginManager>,
    ) -> Result<(), String> {
        for (ticket, entry) in &mut self.resource_cleanups {
            if matches!(entry.phase, Phase::Running) {
                let session = sessions
                    .iter_mut()
                    .find(|session| session.id == entry.engine)
                    .ok_or("retirement session disappeared")?;
                let Some(mut owner) = session
                    .pending_resource_retirements
                    .remove(&entry.operation)
                else {
                    return Err("retirement owner disappeared before completion".into());
                };
                let outcome = owner.outcome().map(|outcome| {
                    if matches!(outcome, OperationOutcome::Succeeded) {
                        if let Err(reason) = owner.finish_metadata(&mut session.borrow_mut()) {
                            return OperationOutcome::Uncertain { reason };
                        }
                    }
                    outcome
                });
                let lease = owner.lease().cloned();
                session
                    .pending_resource_retirements
                    .insert(entry.operation.clone(), owner);
                if let Some(outcome) = outcome {
                    let lease = lease.ok_or("retirement has no Running lease")?;
                    entry.phase = Phase::Finish {
                        uncertain: matches!(outcome, OperationOutcome::Uncertain { .. }),
                    };
                    entry.queued = Some(Work::RetirementFinished { lease, outcome });
                }
            }
            if matches!(entry.phase, Phase::Reconcile)
                && std::time::Instant::now() >= entry.next_reconcile
            {
                entry.next_reconcile =
                    std::time::Instant::now() + std::time::Duration::from_secs(1);
                let session = sessions
                    .iter_mut()
                    .find(|session| session.id == entry.engine)
                    .ok_or("reconciling owner disappeared")?;
                let mut owner = session
                    .pending_resource_retirements
                    .remove(&entry.operation)
                    .ok_or("retirement receipt owner disappeared")?;
                owner.retry_owned(&mut session.borrow_mut(), plugins.as_deref_mut());
                if matches!(owner.outcome(), Some(OperationOutcome::Succeeded)) {
                    if let Err(reason) = owner.finish_metadata(&mut session.borrow_mut()) {
                        tracing::warn!("retirement metadata still requires recovery: {reason}");
                    }
                    if let Some(evidence) = owner.reconciliation_evidence() {
                        let lease = owner
                            .lease()
                            .cloned()
                            .ok_or("reconciling owner has no lease")?;
                        entry.queued = Some(Work::ReconcileRetirement { lease, evidence });
                        entry.phase = Phase::Reconciled;
                    }
                }
                session
                    .pending_resource_retirements
                    .insert(entry.operation.clone(), owner);
            }
            let Some(work) = entry.queued.take() else {
                continue;
            };
            match self.worker.submit_owned(Request {
                ticket: *ticket,
                work,
            }) {
                Ok(()) => {}
                Err((crate::runtime::journal_product::SubmitError::Busy, request)) => {
                    entry.queued = Some(request.work)
                }
                Err((error, _)) => {
                    return Err(format!("committed cleanup submission failed: {error:?}"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn answer_resource_cleanup(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
        sessions: &mut [&mut EngineSession],
        plugins: Option<&mut crate::plugin::PluginManager>,
    ) -> Result<bool, String> {
        let Some(entry) = self.resource_cleanups.get_mut(&ticket) else {
            return Ok(false);
        };
        let session = sessions
            .iter_mut()
            .find(|session| session.id == entry.engine)
            .ok_or("retirement session disappeared")?;
        match entry.phase {
            Phase::Claim => {
                let ResultValue::RetirementClaimed(claim) =
                    result.as_ref().map_err(Clone::clone)?
                else {
                    return Err("cleanup claim returned another result".into());
                };
                if claim.lease.operation != entry.operation {
                    return Err("cleanup completion belongs to another operation".into());
                }
                let mut owner = session
                    .pending_resource_retirements
                    .remove(&entry.operation)
                    .ok_or("retirement resources missing")?;
                let start = owner.start(claim.clone(), &mut session.borrow_mut(), plugins);
                let has_lease = owner.lease().is_some();
                session
                    .pending_resource_retirements
                    .insert(entry.operation.clone(), owner);
                if let Err(error) = start {
                    if !has_lease {
                        return Err(error);
                    }
                    // Partial host publication cannot be reported as a known cancelled close.
                    entry.phase = Phase::Finish { uncertain: true };
                    entry.queued = Some(Work::RetirementFinished {
                        lease: claim.lease.clone(),
                        outcome: OperationOutcome::Uncertain { reason: error },
                    });
                } else {
                    entry.phase = Phase::Running;
                }
                (self.wake)();
            }
            Phase::Finish { uncertain } => {
                match result.as_ref().map_err(Clone::clone)? {
                    ResultValue::Executed(_) | ResultValue::Stored(_) => {}
                    _ => return Err("cleanup result commit returned another value".into()),
                }
                if uncertain {
                    entry.phase = Phase::Reconcile;
                    entry.next_reconcile =
                        std::time::Instant::now() + std::time::Duration::from_secs(1);
                    self.refresh_in_progress_commands();
                    return Ok(true);
                }
                if let Some(owner) = session
                    .pending_resource_retirements
                    .remove(&entry.operation)
                {
                    owner.notify_completed(&mut session.borrow_mut());
                }
                self.resource_cleanups.remove(&ticket);
                self.refresh_in_progress_commands();
            }
            Phase::Reconciled => {
                match result.as_ref().map_err(Clone::clone)? {
                    ResultValue::Executed(_) | ResultValue::Stored(_) => {}
                    _ => return Err("retirement reconciliation returned another value".into()),
                }
                if let Some(owner) = session
                    .pending_resource_retirements
                    .remove(&entry.operation)
                {
                    owner.notify_completed(&mut session.borrow_mut());
                }
                self.resource_cleanups.remove(&ticket);
                self.refresh_in_progress_commands();
            }
            Phase::Reconcile => {
                return Err("unexpected completion during receipt reconciliation".into());
            }
            Phase::Running => {
                return Err("unexpected completion while awaiting cleanup receipts".into());
            }
        }
        Ok(true)
    }
}
