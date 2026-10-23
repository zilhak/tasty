use super::*;

#[derive(Clone, Copy)]
pub(in crate::runtime::journal_product) enum Outcome {
    Ok,
    Error,
    Deferred,
    Lost,
}

impl Outcome {
    pub(in crate::runtime::journal_product) fn waiting(before: usize, after: usize) -> Self {
        if after > before {
            Self::Deferred
        } else {
            Self::Error
        }
    }
    pub(in crate::runtime::journal_product) fn handled(success: bool) -> Self {
        if success { Self::Ok } else { Self::Error }
    }
}

pub(super) struct Entry {
    pub id: u64,
    pub submitted: Instant,
    pub bytes: u64,
    pub accepted: bool,
    pub started: Option<Instant>,
    pub execution_recorded: bool,
    pub outcome: Option<Outcome>,
}

pub(in crate::runtime::journal_product) struct Observation {
    pub(in crate::runtime::journal_product) metrics: Arc<JournalMetrics>,
    id: u64,
    submitted: Instant,
    bytes: u64,
}
impl JournalMetrics {
    pub(in crate::runtime::journal_product) fn prepare(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Observation {
        let mut s = self.lock();
        s.next += 1;
        let id = s.next;
        let submitted = self.clock.now();
        if let Some(slot) = s.entries.iter_mut().find(|e| e.is_none()) {
            *slot = Some(Entry {
                id,
                submitted,
                bytes: bytes as u64,
                accepted: false,
                started: None,
                execution_recorded: false,
                outcome: None,
            });
        } else {
            self.overflow.fetch_add(1, Ordering::Relaxed);
        }
        Observation {
            metrics: self.clone(),
            id,
            submitted,
            bytes: bytes as u64,
        }
    }
    pub(in crate::runtime::journal_product) fn accept(&self, id: u64) {
        let mut s = self.lock();
        let Some(index) = s
            .entries
            .iter()
            .position(|e| e.as_ref().is_some_and(|e| e.id == id))
        else {
            return;
        };
        let entry = s.entries[index].as_mut().expect("located entry");
        if !entry.accepted {
            entry.accepted = true;
            let bytes = entry.bytes;
            s.counts.admitted += 1;
            s.counts.admitted_bytes_estimate += bytes;
        }
        settle(&mut s, index);
    }
    pub(in crate::runtime::journal_product) fn reject(
        &self,
        id: Option<u64>,
        error: &super::super::SubmitError,
    ) {
        let mut s = self.lock();
        match error {
            super::super::SubmitError::Busy => s.counts.rejected_busy += 1,
            super::super::SubmitError::TooLarge => s.counts.rejected_too_large += 1,
            super::super::SubmitError::Stopped => s.counts.rejected_stopped += 1,
        }
        if let Some(id) = id {
            for slot in &mut s.entries {
                if slot.as_ref().is_some_and(|e| e.id == id) {
                    *slot = None;
                    break;
                }
            }
        }
    }
}
impl Observation {
    pub(in crate::runtime::journal_product) fn id(&self) -> u64 {
        self.id
    }
    pub(in crate::runtime::journal_product) fn bytes_estimate(&self) -> u64 {
        self.bytes
    }
    pub(in crate::runtime::journal_product) fn submitted(&self) -> Instant {
        self.submitted
    }
    pub(in crate::runtime::journal_product) fn start(&self) {
        self.metrics.accept(self.id);
        let mut s = self.metrics.lock();
        if let Some(e) = s.entries.iter_mut().flatten().find(|e| e.id == self.id)
            && e.started.is_none()
        {
            let now = self.metrics.clock.now();
            e.started = Some(now);
            self.metrics
                .queue_wait
                .record(now.saturating_duration_since(e.submitted));
        }
    }
    pub(in crate::runtime::journal_product) fn execution_finished(&self) {
        let mut s = self.metrics.lock();
        if let Some(e) = s.entries.iter_mut().flatten().find(|e| e.id == self.id) {
            record_execution(&self.metrics, e);
        }
    }
    pub(in crate::runtime::journal_product) fn finish(&self, outcome: Outcome) {
        let mut s = self.metrics.lock();
        if let Some(index) = s
            .entries
            .iter()
            .position(|e| e.as_ref().is_some_and(|e| e.id == self.id))
        {
            let e = s.entries[index].as_mut().expect("located entry");
            if e.outcome.is_none() {
                record_execution(&self.metrics, e);
                e.outcome = Some(outcome);
                self.metrics.completion.record(
                    self.metrics
                        .clock
                        .now()
                        .saturating_duration_since(e.submitted),
                );
            }
            settle(&mut s, index);
        }
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        self.finish(Outcome::Lost);
    }
}
fn record_execution(metrics: &JournalMetrics, e: &mut Entry) {
    if !e.execution_recorded
        && let Some(start) = e.started
    {
        metrics
            .execution
            .record(metrics.clock.now().saturating_duration_since(start));
        e.execution_recorded = true;
    }
}
fn settle(s: &mut State, index: usize) {
    let e = s.entries[index].as_ref().expect("located entry");
    if !e.accepted {
        return;
    }
    let Some(outcome) = e.outcome else {
        return;
    };
    match outcome {
        Outcome::Ok => s.counts.handled_ok += 1,
        Outcome::Error => s.counts.handled_error += 1,
        Outcome::Deferred => s.counts.deferred += 1,
        Outcome::Lost => s.counts.lost += 1,
    }
    s.entries[index] = None;
}
