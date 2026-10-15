//! Installation outcome and the observation of a replaced owner after publication.
use super::*;

impl Creation {
    pub(in crate::app::journal) fn set_origin_command(&mut self, command: String) {
        self.origin_command = Some(command);
    }
    pub(in crate::app::journal) fn holds_command(&self, command: &str) -> bool {
        self.origin_command.as_deref() == Some(command)
    }
    pub(in crate::app::journal) fn is_settled(&self) -> bool {
        matches!(self.stage, Stage::Settled { .. })
    }
    /// The origin command and the reason its replaced owner was not confirmed retired.
    pub(in crate::app::journal) fn unconfirmed_cleanup(&self) -> Option<(&str, &str)> {
        match &self.stage {
            Stage::Settled {
                unconfirmed: Some(reason),
            } => Some((self.origin_command.as_deref()?, reason)),
            _ => None,
        }
    }
    /// Finished publication leaves the creation; outstanding receipts keep it as Settling.
    pub(super) fn settle(installed: Installed) -> Option<Stage> {
        if matches!(installed.release_complete(), Ok(true)) {
            None
        } else {
            Some(Stage::Settling {
                installed,
                deadline: std::time::Instant::now() + SETTLE_DEADLINE,
            })
        }
    }
    /// An assembly member leaves after the batch ACK unless replaced owners still need receipts.
    pub(in crate::app::journal) fn release_published(&mut self) -> bool {
        if !matches!(self.stage, Stage::Published(_)) {
            return false;
        }
        let Stage::Published(installed) = std::mem::replace(&mut self.stage, Stage::Transition)
        else {
            unreachable!("checked published stage");
        };
        match Self::settle(installed) {
            Some(stage) => {
                self.stage = stage;
                false
            }
            None => {
                self.stage = Stage::Settled { unconfirmed: None };
                true
            }
        }
    }

    /// A failed install still owns any candidate handles and already-issued retirement receipts.
    /// A failure before any owner changed is a known rejection of the untouched candidate. A
    /// later failure withdraws the unpublished new owner and leaves only that target unavailable.
    pub(in crate::app::journal) fn install_candidate(
        &mut self,
        session: &mut EngineSession,
        mut plugins: Option<&mut crate::plugin::PluginManager>,
        retiring: Option<effect_runner::RetiringKind>,
        mut installation: effect_runner::Installation,
    ) -> Result<(), String> {
        let preparation_acked = matches!(
            self.stage,
            Stage::Prepared {
                acknowledged: true,
                ..
            }
        );
        let error =
            match installation.install(&mut session.borrow_mut(), plugins.as_deref_mut(), retiring)
            {
                Ok(installed) => {
                    self.installed(installed);
                    return Ok(());
                }
                Err(failure) => failure,
            };
        let reason = error.error.to_string();
        let lease = installation.lease.clone();
        let surface = installation.surface_id();
        tracing::warn!(
            operation = %lease.operation.0,
            stream = %lease.stream,
            surface,
            attempt = lease.attempt,
            generation = lease.resource_generation,
            phase = "install",
            applied = error.applied,
            "creation installation failed: {reason}"
        );
        let candidate = session.pending_materializations.remove(&lease.operation);
        if !error.applied {
            let mut candidate = candidate.ok_or("rejected installation lost its candidate")?;
            candidate.revert_installation(installation);
            let retirement = match self.discard_candidate(session, candidate)? {
                Some(receipt) => receipt,
                None => return Ok(()),
            };
            self.stage = Stage::Rejected {
                discard_committed: false,
                lease,
                reason,
                retirement,
                answered: preparation_acked,
                started: std::time::Instant::now(),
            };
            return Ok(());
        }
        let old_retired = installation.withdraw_unpublished(&mut session.borrow_mut(), plugins);
        let mut surfaces: Vec<Box<dyn crate::model::Surface>> = candidate
            .map(|candidate| candidate.leaf.surface)
            .into_iter()
            .collect();
        if old_retired {
            surfaces.extend(mark_unavailable(session, surface, &reason));
        }
        // The stage keeps the owner even if the record cannot be queued.
        self.stage = Stage::Uncertain(Uncertain::pending(
            reason.clone(),
            UncertainOwner::FailedInstallation(Box::new(uncertain::Withdrawn {
                installation,
                surfaces,
            })),
        ));
        self.enqueue(Work::PreparationUncertain { lease, reason })?;
        Ok(())
    }

    /// The structure is published; observes the replaced owner without pausing observation.
    /// Returns true while this stage handled the poll.
    pub(in crate::app::journal) fn poll_settling(&mut self, session: &mut EngineSession) -> bool {
        if let Stage::Settling {
            installed,
            deadline,
        } = &self.stage
        {
            let unconfirmed = match installed.release_complete() {
                Ok(true) => None,
                Ok(false) if std::time::Instant::now() < *deadline => return true,
                Ok(false) => {
                    Some("replaced owner did not confirm retirement before its deadline".to_owned())
                }
                Err(error) => Some(error.to_string()),
            };
            let Stage::Settling { installed, .. } =
                std::mem::replace(&mut self.stage, Stage::Transition)
            else {
                unreachable!("checked settling stage")
            };
            if let Some(reason) = &unconfirmed {
                tracing::warn!(
                    operation = %installed.lease.operation.0,
                    stream = %installed.lease.stream,
                    surface = installed.surface_id,
                    attempt = installed.lease.attempt,
                    generation = installed.lease.resource_generation,
                    phase = "settle",
                    "published creation kept an unconfirmed replaced owner: {reason}"
                );
                // The engine release is the single later owner; a timeout is not a reap.
                session
                    .engine_release
                    .get_or_insert_with(Default::default)
                    .retain_installation(installed);
            }
            self.stage = Stage::Settled { unconfirmed };
            return true;
        }
        false
    }
}

/// A published target whose old owner was already retired by a failed replacement shows an
/// explicit recovery-blocked placeholder instead of a missing resource. Returns the replaced object.
fn mark_unavailable(
    session: &mut EngineSession,
    surface: u32,
    reason: &str,
) -> Option<Box<dyn crate::model::Surface>> {
    let descriptor = session.core_state.find_surface_by_id(surface)?;
    let placeholder = crate::runtime::surface_restorer::JournalPlaceholder {
        id: surface,
        kind: descriptor.kind.clone(),
        data: None,
        creation_seed: None,
        activation: descriptor
            .activation_generation
            .map(|generation| tasty_core::Activation {
                generation,
                phase: tasty_core::ActivationPhase::Ready,
            }),
        attempts: 0,
        failure: Some(reason.to_owned()),
        recovery_blocked: true,
    };
    session
        .runtime
        .surfaces
        .insert(surface, Box::new(placeholder))
}
