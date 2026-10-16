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
    /// Preparation produced no local owner.
    EngineOwned,
    /// A standalone transfer that could not return to its original registration.
    Unreturned(Box<effect_runner::PreparedMaterialization>),
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
            Self::Unreturned(candidate) => candidate.release_keeping_standalone(release),
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

/// How long a committed uncertainty keeps observing its owner's receipts. Afterwards the owner
/// moves to the engine release and the durable outcome stays unknown for restart recovery.
const RECONCILE_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

// The journal ACK is a prerequisite for observing and retrying the external effect.
enum Reconciliation {
    AwaitingCommit {
        not_before: std::time::Instant,
    },
    Committed {
        next: std::time::Instant,
        until: std::time::Instant,
    },
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
            self.phase = Reconciliation::Committed {
                next: not_before,
                until: std::time::Instant::now() + RECONCILE_WINDOW,
            };
        }
    }
    /// No receipt or state notice can ever reconcile this owner. An adopted standalone process
    /// that could not return to its registration has no receipt either, since nothing retires it.
    pub(super) fn has_no_evidence(&self) -> bool {
        match &self.owner {
            UncertainOwner::EngineOwned | UncertainOwner::Unreturned(_) => true,
            UncertainOwner::FailedInstallation(withdrawn) => {
                withdrawn.installation.retains_adopted_standalone()
            }
            _ => false,
        }
    }
    /// Before the ACK nothing can be polled; the worker's reply wakes the App by itself.
    pub(super) fn deadline(&self) -> Option<std::time::Instant> {
        match self.phase {
            Reconciliation::AwaitingCommit { .. } => None,
            Reconciliation::Committed { next, until } => Some(next.min(until)),
        }
    }
    fn begin_poll(&mut self, now: std::time::Instant) -> bool {
        let Reconciliation::Committed { next, .. } = &mut self.phase else {
            return false;
        };
        if now < *next {
            return false;
        }
        *next = now + std::time::Duration::from_secs(1);
        true
    }
    fn window_closed(&self, now: std::time::Instant) -> bool {
        matches!(self.phase, Reconciliation::Committed { until, .. } if now >= until)
    }
    pub(super) fn retain_for_release(
        self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        self.owner.retain_for_release(release);
    }
}

/// One record per entry into uncertainty. The reason never carries input bodies.
pub(super) fn log_uncertain(
    lease: &crate::runtime::journal_product::EffectLease,
    phase: &str,
    reason: &str,
) {
    tracing::warn!(
        operation = %lease.operation.0,
        stream = %lease.stream,
        attempt = lease.attempt,
        generation = lease.resource_generation,
        phase,
        "creation outcome became uncertain: {reason}"
    );
}

impl Creation {
    #[cfg(test)]
    pub(in crate::app::journal) fn uncertain_reason(&self) -> Option<&str> {
        match &self.stage {
            Stage::Uncertain(uncertain) => Some(&uncertain.reason),
            _ => None,
        }
    }
    /// The committed uncertainty ends this continuation: its owner, if any, moves to the engine
    /// release. The durable outcome stays unknown, so the held reply reads it as recovery-required.
    pub(super) fn leave_uncertain(&mut self, session: &mut EngineSession) {
        let Stage::Uncertain(uncertain) = std::mem::replace(&mut self.stage, Stage::Transition)
        else {
            unreachable!("only an uncertain creation leaves as uncertain")
        };
        self.receipt
            .get_or_init(|| ActivationOutcome::Failed(uncertain.reason.clone()));
        uncertain.retain_for_release(session.engine_release.get_or_insert_with(Default::default));
        self.stage = Stage::Settled { unconfirmed: None };
    }
    pub(super) fn retain_uncertain(&mut self, reason: String) {
        let owner =
            UncertainOwner::from_stage(std::mem::replace(&mut self.stage, Stage::Transition))
                .unwrap_or(UncertainOwner::EngineOwned);
        self.stage = Stage::Uncertain(Uncertain::pending(reason, owner));
    }
    pub(in crate::app::journal) fn cleanup_deadline(&self) -> Option<std::time::Instant> {
        if let Stage::Uncertain(uncertain) = &self.stage {
            uncertain.deadline()
        } else if let Some(deadline) = self.settling_deadline() {
            Some(deadline)
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
        let now = std::time::Instant::now();
        let work = if uncertain.begin_poll(now) {
            self.reconciliation_work(view, session)?
        } else {
            None
        };
        let Some(work) = work else {
            if matches!(&self.stage, Stage::Uncertain(uncertain) if uncertain.window_closed(now)) {
                self.expire_uncertain(session);
            }
            return self.flush(worker).map(|_| ());
        };
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
        self.flush(worker)?;
        Ok(())
    }

    fn expire_uncertain(&mut self, session: &mut EngineSession) {
        if let Stage::Uncertain(uncertain) = &self.stage {
            tracing::warn!(
                stream = %self.binding.stream,
                ticket = self.ticket,
                "uncertain creation left its owner to the engine release: {}",
                uncertain.reason
            );
        }
        self.leave_uncertain(session);
    }

    /// Builds the reconciliation from the retained owner's receipts, if they are complete.
    fn reconciliation_work(
        &self,
        view: crate::runtime::journal_product::CompletionView,
        session: &EngineSession,
    ) -> Result<Option<Work>, String> {
        let Stage::Uncertain(uncertain) = &self.stage else {
            return Ok(None);
        };
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
        Ok(work)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Only the worker's ACK moves an unacknowledged record on, and that reply wakes the App.
    /// A deadline that has already passed would wake every turn until it arrives.
    #[test]
    fn an_unacknowledged_record_asks_for_no_past_wake_up() {
        let mut state = Uncertain::pending("waiting".into(), UncertainOwner::EngineOwned);
        state.phase = Reconciliation::AwaitingCommit {
            not_before: std::time::Instant::now() - std::time::Duration::from_secs(1),
        };
        let now = std::time::Instant::now();
        assert!(
            state.deadline().is_none_or(|deadline| deadline > now),
            "a past deadline: {:?}",
            state.deadline()
        );
    }
    #[test]
    fn reconciliation_requires_commit_and_preserves_the_original_deadline() {
        let mut state = Uncertain::pending("waiting".into(), UncertainOwner::EngineOwned);
        let Reconciliation::AwaitingCommit { not_before: due } = state.phase else {
            unreachable!("a new record awaits its commit")
        };
        assert_eq!(state.deadline(), None);
        assert!(!state.begin_poll(due + std::time::Duration::from_secs(10)));
        assert!(!state.window_closed(due + std::time::Duration::from_secs(10)));
        state.acknowledge();
        assert_eq!(state.deadline(), Some(due));
        assert!(!state.begin_poll(due - std::time::Duration::from_millis(1)));
        assert!(state.begin_poll(due));
        let next = state.deadline().unwrap();
        state.acknowledge();
        assert_eq!(
            state.deadline(),
            Some(next),
            "duplicate ACK must not reset retry timing"
        );
        assert!(!state.begin_poll(due));
        assert!(state.begin_poll(next));
    }
}
