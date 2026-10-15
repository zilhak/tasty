//! Preparation of a claimed candidate. A terminal child starts on the spawner while the App keeps
//! observing; the candidate stays private until the spawned result meets its engine again.
use super::*;
use crate::runtime::effect_runner::{
    Begun, PendingCandidate, PendingSpawn, PreparationFailure, PreparedMaterialization,
    SpawnCompletion, TerminalSpawner,
};

/// Upper bound for observing the reap of a stale spawned child before it becomes uncertain.
const DISCARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// A candidate whose terminal child is starting, or a stale child awaiting its reap.
pub(super) enum Preparing {
    Spawning {
        candidate: PendingCandidate,
        pending: PendingSpawn,
    },
    Discarding {
        lease: crate::runtime::journal_product::EffectLease,
        reason: String,
        retirement: tasty_terminal::PtyRetirement,
        started: std::time::Instant,
    },
}

impl Preparing {
    /// The engine release owns whatever the spawner eventually returns.
    pub(super) fn retain_for_release(
        self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        match self {
            Self::Spawning { pending, .. } => release.retain_spawn(pending),
            Self::Discarding { retirement, .. } => release.retain_pty(retirement),
        }
    }
}

impl Creation {
    #[cfg(test)]
    pub(in crate::app::journal) fn is_preparing(&self) -> bool {
        matches!(self.stage, Stage::Preparing(Preparing::Spawning { .. }))
    }

    pub(super) fn begin_preparation(
        &mut self,
        worker: &JournalWorker,
        session: &mut EngineSession,
        spawner: &TerminalSpawner,
        claimed: crate::runtime::journal_product::ClaimedPreparation,
    ) -> Result<Stage, String> {
        let lease = claimed.lease.clone();
        let binding = ExecutionBinding {
            stream: self.binding.stream.clone(),
            runtime_epoch: self.binding.runtime_epoch,
            engine_incarnation: self.binding.incarnation,
        };
        match effect_runner::prepare(&mut session.borrow_mut(), &binding, claimed) {
            Ok(Begun::Ready(prepared)) => self.accept_prepared(worker, session, *prepared),
            Ok(Begun::Spawning(request)) => {
                let (job, candidate) = (*request).split();
                match spawner.submit(job) {
                    Ok(pending) => Ok(Stage::Preparing(Preparing::Spawning { candidate, pending })),
                    // Nothing executed for a refused job, so the refusal is a known failure.
                    Err((_, reason)) => self.preparation_failed(
                        worker,
                        lease,
                        PreparationFailure {
                            reason,
                            uncertain: false,
                        },
                    ),
                }
            }
            Err(failure) => self.preparation_failed(worker, lease, failure),
        }
    }

    fn accept_prepared(
        &mut self,
        worker: &JournalWorker,
        session: &mut EngineSession,
        mut prepared: PreparedMaterialization,
    ) -> Result<Stage, String> {
        prepared.set_one_shot_input(self.one_shot_input.take());
        let result = if prepared.is_deferred() {
            PreparationResult::Deferred { data: None }
        } else {
            PreparationResult::Ready { data: None }
        };
        let lease = prepared.lease.clone();
        let operation = lease.operation.clone();
        session
            .pending_materializations
            .insert(operation.clone(), prepared);
        self.submit(worker, Work::Prepared { lease, result })?;
        Ok(Stage::Prepared {
            operation,
            acknowledged: false,
        })
    }

    fn preparation_failed(
        &mut self,
        worker: &JournalWorker,
        lease: crate::runtime::journal_product::EffectLease,
        failure: PreparationFailure,
    ) -> Result<Stage, String> {
        let reason = failure.reason;
        if failure.uncertain {
            super::uncertain::log_uncertain(&lease, "prepare", &reason);
            self.submit(
                worker,
                Work::PreparationUncertain {
                    lease,
                    reason: reason.clone(),
                },
            )?;
            return Ok(Stage::Uncertain(Uncertain::pending(
                reason,
                UncertainOwner::EngineOwned,
            )));
        }
        self.submit(
            worker,
            Work::Prepared {
                lease,
                result: PreparationResult::Failed {
                    reason: reason.clone(),
                },
            },
        )?;
        Ok(Stage::Failed(reason))
    }

    /// Returns true while a preparation stage handled this poll.
    pub(super) fn poll_preparing(
        &mut self,
        worker: &JournalWorker,
        session: &mut EngineSession,
    ) -> Result<bool, String> {
        let Stage::Preparing(preparing) = &self.stage else {
            return Ok(false);
        };
        match preparing {
            Preparing::Spawning { pending, .. } => {
                let Some(outcome) = pending.try_take() else {
                    return Ok(true);
                };
                let Stage::Preparing(Preparing::Spawning { candidate, .. }) =
                    std::mem::replace(&mut self.stage, Stage::Transition)
                else {
                    unreachable!("checked spawning stage");
                };
                self.stage = self.spawned(worker, session, candidate, outcome)?;
            }
            Preparing::Discarding {
                lease,
                reason,
                retirement,
                started,
            } => {
                let phase = retirement.observation().phase;
                if phase == tasty_terminal::PtyPhase::Reaped {
                    let (lease, reason) = (lease.clone(), reason.clone());
                    let failure = PreparationFailure {
                        reason,
                        uncertain: false,
                    };
                    self.stage = self.preparation_failed(worker, lease, failure)?;
                } else if phase == tasty_terminal::PtyPhase::WaitFailed
                    || started.elapsed() >= DISCARD_DEADLINE
                {
                    let lease = lease.clone();
                    let reason = format!("{reason}; its started child could not confirm reap");
                    let Stage::Preparing(Preparing::Discarding { retirement, .. }) =
                        std::mem::replace(&mut self.stage, Stage::Transition)
                    else {
                        unreachable!("checked discarding stage");
                    };
                    let release = session.engine_release.get_or_insert_with(Default::default);
                    release.retain_pty(retirement);
                    let failure = PreparationFailure {
                        reason,
                        uncertain: true,
                    };
                    self.stage = self.preparation_failed(worker, lease, failure)?;
                }
            }
        }
        self.flush(worker)?;
        Ok(true)
    }

    fn spawned(
        &mut self,
        worker: &JournalWorker,
        session: &mut EngineSession,
        candidate: PendingCandidate,
        outcome: effect_runner::SpawnOutcome,
    ) -> Result<Stage, String> {
        let lease = candidate.lease().clone();
        let spawned = match outcome {
            Ok(spawned) => spawned,
            Err(reason) => {
                // The failure may follow a started child; preparation_failed logs it once.
                let failure = PreparationFailure {
                    reason,
                    uncertain: true,
                };
                return self.preparation_failed(worker, lease, failure);
            }
        };
        match candidate.complete(&session.borrow_mut(), spawned) {
            SpawnCompletion::Ready(prepared) => self.accept_prepared(worker, session, *prepared),
            SpawnCompletion::Stale {
                lease,
                reason,
                retirement,
            } => {
                tracing::warn!(
                    operation = %lease.operation.0,
                    stream = %lease.stream,
                    attempt = lease.attempt,
                    generation = lease.resource_generation,
                    "spawned terminal candidate discarded: {reason}"
                );
                Ok(Stage::Preparing(Preparing::Discarding {
                    lease,
                    reason,
                    retirement,
                    started: std::time::Instant::now(),
                }))
            }
        }
    }
}
