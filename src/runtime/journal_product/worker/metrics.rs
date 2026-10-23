//! Optional per-work timing; no shared AppServices/diagnostics wiring or scheduling changes.
use std::time::Instant;

use tasty_event_store::CommitStatistics;

use super::{Executor, StructureDecider, Work};

pub(super) struct Measurement {
    submitted: Instant,
    kind: &'static str,
    before: Option<CommitStatistics>,
}

impl Measurement {
    pub(super) fn begin(
        executor: &Executor<StructureDecider>,
        submitted: Instant,
        work: &Work,
    ) -> Self {
        let before = tracing::enabled!(target: "tasty::journal_metrics", tracing::Level::DEBUG)
            .then(|| snapshot(executor))
            .flatten();
        Self {
            submitted,
            kind: kind(work),
            before,
        }
    }

    pub(super) fn durable(
        &self,
        executor: &Executor<StructureDecider>,
        ticket: u64,
        success: bool,
    ) {
        let Some(before) = self.before else {
            return;
        };
        let Some(after) = snapshot(executor) else {
            return;
        };
        tracing::debug!(target: "tasty::journal_metrics", phase="durable_ready", ticket,
            work=self.kind, success, queue_to_ready_us=self.submitted.elapsed().as_micros() as u64,
            physical=after.physical_commits-before.physical_commits,
            structural=after.structural_commits-before.structural_commits,
            logical=after.logical_requests-before.logical_requests,
            sql_commit_ns=after.commit_ns-before.commit_ns,
            physical_total=after.physical_commits, structural_total=after.structural_commits,
            "journal work measurement");
    }

    pub(super) fn finished(
        &self,
        executor: &Executor<StructureDecider>,
        ticket: u64,
        success: bool,
    ) {
        if let Some(before) = self.before
            && let Some(after) = snapshot(executor)
        {
            tracing::debug!(target: "tasty::journal_metrics", phase="finished", ticket,
                work=self.kind, success, queue_to_finished_us=self.submitted.elapsed().as_micros() as u64,
                physical=after.physical_commits-before.physical_commits,
                structural=after.structural_commits-before.structural_commits,
                logical=after.logical_requests-before.logical_requests,
                physical_total=after.physical_commits, structural_total=after.structural_commits,
                "journal work measurement");
        }
    }
}

fn snapshot(executor: &Executor<StructureDecider>) -> Option<CommitStatistics> {
    match executor.inner.lock() {
        Ok(inner) => Some(inner.store.commit_statistics()),
        Err(error) => {
            tracing::warn!("journal measurement unavailable: {error}");
            None
        }
    }
}

fn kind(work: &Work) -> &'static str {
    match work {
        Work::Resolve { .. } => "resolve",
        #[cfg(feature = "gui")]
        Work::ForwardFinished { .. } => "effect_result",
        #[cfg(feature = "gui")]
        Work::ClaimForward { .. } => "barrier_claim",
        Work::Prepared { .. }
        | Work::CleanupFinished { .. }
        | Work::RetirementFinished { .. }
        | Work::InstallationRejected { .. }
        | Work::PreparationUncertain { .. } => "effect_result",
        Work::Capture { .. } | Work::CaptureClosed { .. } => "barrier_capture",
        Work::Reserve(..) | Work::ReserveExecutionIds { .. } => "barrier_reserve",
        Work::ClaimPreparation { .. } | Work::ClaimRetirement { .. } => "barrier_claim",
        _ => "barrier_other",
    }
}

pub(super) fn total(executor: &Executor<StructureDecider>, phase: &'static str) {
    if tracing::enabled!(target: "tasty::journal_metrics", tracing::Level::DEBUG)
        && let Some(stats) = snapshot(executor)
    {
        tracing::debug!(target: "tasty::journal_metrics", phase,
            physical_total=stats.physical_commits, structural_total=stats.structural_commits,
            logical_total=stats.logical_requests, failed_commits=stats.failed_commits,
            sql_commit_ns=stats.commit_ns, "journal transaction totals");
    }
}

/// Counter interval for one physical group, excluding later admission release/publication.
pub(super) struct GroupMeasurement {
    before: Option<CommitStatistics>,
    started: Instant,
}
impl GroupMeasurement {
    pub(super) fn begin(executor: &Executor<StructureDecider>) -> Self {
        Self {
            before: tracing::enabled!(target: "tasty::journal_metrics", tracing::Level::DEBUG)
                .then(|| snapshot(executor))
                .flatten(),
            started: Instant::now(),
        }
    }

    pub(super) fn finished(
        &self,
        executor: &Executor<StructureDecider>,
        requests: usize,
        resolves: usize,
        halted: bool,
    ) {
        let Some(before) = self.before else {
            return;
        };
        let Some(after) = snapshot(executor) else {
            return;
        };
        tracing::debug!(target: "tasty::journal_metrics", phase="group_finished", requests,
            resolve_requests=resolves, effect_results=requests-resolves, halted,
            physical=after.physical_commits-before.physical_commits,
            structural=after.structural_commits-before.structural_commits,
            logical=after.logical_requests-before.logical_requests,
            failed_commits=after.failed_commits-before.failed_commits,
            sql_commit_ns=after.commit_ns-before.commit_ns,
            group_ready_us=self.started.elapsed().as_micros() as u64,
            physical_total=after.physical_commits, structural_total=after.structural_commits,
            "journal group transaction");
    }
}
