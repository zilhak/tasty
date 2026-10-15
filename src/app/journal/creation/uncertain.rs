//! Uncertain materialization keeps its exact owner until reconciliation or engine release.
use super::*;

/// A partially applied installation after its unpublished new owner was withdrawn. The kind
/// objects (the private leaf and any replaced published one) end after their receipts.
pub(in crate::app::journal) struct Withdrawn {
    pub(in crate::app::journal) installation: effect_runner::Installation,
    pub(in crate::app::journal) surfaces: Vec<Box<dyn crate::model::Surface>>,
}

/// An uncertain transition retains either the installed owner or the rejected candidate receipt.
pub(super) enum UncertainOwner {
    /// The candidate is retained by EngineSession, or preparation produced no local owner.
    EngineOwned,
    FailedInstallation(Box<Withdrawn>),
    Installed(Installed),
    Discard {
        lease: crate::runtime::journal_product::EffectLease,
        retirement: Option<tasty_terminal::PtyRetirement>,
        discard_committed: bool,
        reason: String,
    },
}

impl UncertainOwner {
    pub(super) fn from_stage(stage: Stage) -> Option<Self> {
        match stage {
            Stage::Installing { installed, .. }
            | Stage::Finish { installed, .. }
            | Stage::AwaitPublication(installed)
            | Stage::Published(installed)
            | Stage::Settling { installed, .. } => Some(Self::Installed(installed)),
            Stage::Rejected {
                lease,
                retirement,
                discard_committed,
                reason,
                ..
            } => Some(Self::Discard {
                lease,
                retirement,
                discard_committed,
                reason,
            }),
            Stage::Uncertain(uncertain) => Some(uncertain.owner),
            _ => None,
        }
    }

    pub(super) fn retain_for_release(
        self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        match self {
            Self::EngineOwned => {}
            Self::FailedInstallation(withdrawn) => {
                let Withdrawn {
                    installation,
                    surfaces,
                } = *withdrawn;
                installation.retire_for_release(release);
                for surface in surfaces {
                    release.retain_surface(surface);
                }
            }
            Self::Installed(installed) => release.retain_installation(installed),
            Self::Discard {
                retirement: Some(receipt),
                ..
            } => release.retain_pty(receipt),
            Self::Discard {
                retirement: None, ..
            } => {}
        }
    }
}

pub(super) struct Uncertain {
    pub(super) reason: String,
    owner: UncertainOwner,
    phase: Reconciliation,
}

// The journal ACK is a prerequisite for observing and retrying the external effect.
enum Reconciliation {
    AwaitingCommit { not_before: std::time::Instant },
    Committed { next: std::time::Instant },
}
impl Uncertain {
    pub(super) fn pending(reason: String, owner: UncertainOwner) -> Self {
        Self {
            reason,
            owner,
            phase: Reconciliation::AwaitingCommit {
                not_before: std::time::Instant::now() + std::time::Duration::from_secs(1),
            },
        }
    }
    /// An installed owner that has not been published still differs from the live structure.
    pub(super) fn holds_unpublished_installation(&self) -> bool {
        matches!(self.owner, UncertainOwner::Installed(_))
    }
    pub(super) fn acknowledge(&mut self) {
        if let Reconciliation::AwaitingCommit { not_before } = self.phase {
            self.phase = Reconciliation::Committed { next: not_before };
        }
    }
    pub(super) fn deadline(&self) -> std::time::Instant {
        match self.phase {
            Reconciliation::AwaitingCommit { not_before } => not_before,
            Reconciliation::Committed { next } => next,
        }
    }
    fn begin_poll(&mut self, now: std::time::Instant) -> bool {
        let Reconciliation::Committed { next } = &mut self.phase else {
            return false;
        };
        if now < *next {
            return false;
        }
        *next = now + std::time::Duration::from_secs(1);
        true
    }
    pub(super) fn retain_for_release(
        self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        self.owner.retain_for_release(release);
    }
}

impl Creation {
    pub(super) fn retain_uncertain(&mut self, reason: String) {
        let owner =
            UncertainOwner::from_stage(std::mem::replace(&mut self.stage, Stage::Transition))
                .unwrap_or(UncertainOwner::EngineOwned);
        self.stage = Stage::Uncertain(Uncertain::pending(reason, owner));
    }
    pub(in crate::app::journal) fn cleanup_deadline(&self) -> Option<std::time::Instant> {
        if let Stage::Uncertain(uncertain) = &self.stage {
            Some(uncertain.deadline())
        } else {
            self.needs_cleanup_poll()
                .then(|| std::time::Instant::now() + std::time::Duration::from_millis(10))
        }
    }
    pub(super) fn poll_reconciliation(
        &mut self,
        worker: &JournalWorker,
        view: crate::runtime::journal_product::CompletionView,
        session: &mut EngineSession,
    ) -> Result<(), String> {
        let Stage::Uncertain(uncertain) = &mut self.stage else {
            return Ok(());
        };
        if !uncertain.begin_poll(std::time::Instant::now()) {
            return Ok(());
        }
        // Build the work while borrowing the owner. Submission failure must leave ownership intact.
        let work = match &uncertain.owner {
            UncertainOwner::Installed(installed) => installed
                .reconciliation_evidence(&session.as_ref(), self.binding.incarnation)
                .map(|evidence| Work::ReconcilePreparation {
                    lease: installed.lease.clone(),
                    evidence,
                    discarded: None,
                    view,
                }),
            UncertainOwner::FailedInstallation(withdrawn)
                if matches!(withdrawn.installation.release_complete(), Ok(true)) =>
            {
                let lease = &withdrawn.installation.lease;
                let evidence = serde_json::to_vec(&serde_json::json!({"version":1,"source":"owned-preparation-receipts","runtime_epoch":self.binding.runtime_epoch,"engine_incarnation":self.binding.incarnation,"lease":lease,"installation_withdrawn":true,"discard_complete":true,"surface":withdrawn.installation.surface_id()})).map_err(|error|error.to_string())?;
                Some(Work::ReconcilePreparation {
                    lease: lease.clone(),
                    evidence,
                    discarded: Some(uncertain.reason.clone()),
                    view,
                })
            }
            UncertainOwner::Discard {
                lease,
                retirement,
                discard_committed,
                reason,
            } if retirement.as_ref().is_none_or(|receipt| {
                receipt.observation().phase == tasty_terminal::PtyPhase::Reaped
            }) =>
            {
                let evidence = serde_json::to_vec(&serde_json::json!({"version":1,"source":"owned-preparation-receipts","runtime_epoch":self.binding.runtime_epoch,"engine_incarnation":self.binding.incarnation,"lease":lease,"discard_complete":true,"physical_generation":retirement.as_ref().map(|receipt|receipt.generation().value())})).map_err(|error|error.to_string())?;
                Some(Work::ReconcilePreparation {
                    lease: lease.clone(),
                    evidence,
                    discarded: (!*discard_committed).then(|| reason.clone()),
                    view,
                })
            }
            _ => None,
        };
        if let Some(work) = work {
            self.submit(worker, work)?;
            let Stage::Uncertain(uncertain) = std::mem::replace(&mut self.stage, Stage::Transition)
            else {
                unreachable!("checked uncertain owner");
            };
            self.stage = match uncertain.owner {
                UncertainOwner::Installed(installed) => Stage::Finish {
                    installed,
                    publication: Publication::Pending,
                },
                UncertainOwner::Discard { reason, .. } => Stage::Failed(reason),
                UncertainOwner::FailedInstallation(_) => Stage::Failed(uncertain.reason),
                _ => unreachable!("only evidenced owners can reconcile"),
            };
        }
        self.flush(worker)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconciliation_requires_commit_and_preserves_the_original_deadline() {
        let mut state = Uncertain::pending("waiting".into(), UncertainOwner::EngineOwned);
        let due = state.deadline();
        assert!(!state.begin_poll(due + std::time::Duration::from_secs(10)));
        state.acknowledge();
        assert_eq!(state.deadline(), due);
        assert!(!state.begin_poll(due - std::time::Duration::from_millis(1)));
        assert!(state.begin_poll(due));
        let next = state.deadline();
        state.acknowledge();
        assert_eq!(
            state.deadline(),
            next,
            "duplicate ACK must not reset retry timing"
        );
        assert!(!state.begin_poll(due));
        assert!(state.begin_poll(next));
    }
}
