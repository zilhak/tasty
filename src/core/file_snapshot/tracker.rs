//! Disk revisions for registry exports; effective-registry recomputation has a separate owner.
#[cfg(test)]
mod dirty_age_tests;
use super::{Document, PendingSave, Snapshot, Submit};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tasty_utils::batch_writer::{Clock, SystemClock};

#[derive(Clone, Default)]
pub(crate) struct Ticket(Arc<Mutex<Option<Result<(), String>>>>);
impl Ticket {
    pub(crate) fn poll(&self) -> Option<Result<(), String>> {
        self.0.lock().unwrap().clone()
    }
    fn finish(&self, result: Result<(), String>) {
        *self.0.lock().unwrap() = Some(result);
    }
}

struct Pending {
    revision: u64,
    receipt: Box<dyn PendingSave>,
    ticket: Ticket,
    notify: bool,
}

pub(crate) struct Tracker {
    key: &'static str,
    document: Arc<Document>,
    state: Mutex<State>,
    clock: Arc<dyn Clock>,
}
#[derive(Default)]
struct State {
    visible: u64,
    committed: u64,
    latest: Option<Vec<u8>>,
    sink: Option<Submit>,
    pending: Vec<Pending>,
    latest_notify: bool,
    errors: Vec<String>,
    retried: bool,
    reported: bool,
    dirty_since: Option<Instant>,
}
impl Tracker {
    pub(crate) fn new(key: &'static str, document: Arc<Document>) -> Self {
        Self::loaded(key, document, 0)
    }
    pub(crate) fn loaded(key: &'static str, document: Arc<Document>, revision: u64) -> Self {
        Self::loaded_with_clock(key, document, revision, Arc::new(SystemClock))
    }
    pub(crate) fn loaded_with_clock(
        key: &'static str,
        document: Arc<Document>,
        revision: u64,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            key,
            document,
            clock,
            state: Mutex::new(State {
                visible: revision,
                committed: revision,
                ..Default::default()
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| {
            tracing::warn!(
                file = self.key,
                "file snapshot lock poisoned; retaining current state"
            );
            poisoned.into_inner()
        })
    }
    pub(crate) fn install_sink(&self, sink: Submit) {
        self.replace_sink(Some(sink));
    }
    pub(crate) fn replace_sink(&self, sink: Option<Submit>) {
        self.lock().sink = sink;
    }
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn update(&self, bytes: Result<Vec<u8>, String>) {
        self.update_with_receipt(bytes, true);
    }
    pub(crate) fn update_with_receipt(
        &self,
        bytes: Result<Vec<u8>, String>,
        notify: bool,
    ) -> Ticket {
        let mut state = self.lock();
        let ticket = Ticket::default();
        state.visible += 1;
        state.dirty_since.get_or_insert_with(|| self.clock.now());
        state.latest_notify = notify;
        match bytes {
            Ok(bytes) => {
                state.latest = Some(bytes);
                self.enqueue(&mut state, ticket.clone());
            }
            Err(error) => {
                state.latest = None;
                ticket.finish(Err(error.clone()));
                if notify {
                    state.errors.push(error);
                }
            }
        }
        ticket
    }
    fn enqueue(&self, state: &mut State, ticket: Ticket) {
        let result = (|| {
            let bytes = state
                .latest
                .clone()
                .ok_or("latest file snapshot could not be serialized")?;
            let sink = state.sink.as_ref().ok_or("file writer is unavailable")?;
            sink(Snapshot {
                key: self.key,
                document: self.document.clone(),
                bytes,
            })
        })();
        match result {
            Ok(receipt) => state.pending.push(Pending {
                revision: state.visible,
                receipt,
                ticket,
                notify: state.latest_notify,
            }),
            Err(error) => {
                ticket.finish(Err(error.clone()));
                if state.latest_notify {
                    state.errors.push(error);
                }
            }
        }
    }
    pub(crate) fn poll(&self) -> Vec<String> {
        let mut state = self.lock();
        let mut index = 0;
        while index < state.pending.len() {
            let Some(result) = state.pending[index].receipt.poll() else {
                index += 1;
                continue;
            };
            let pending = state.pending.swap_remove(index);
            pending.ticket.finish(result.clone());
            match result {
                Ok(()) => state.committed = state.committed.max(pending.revision),
                Err(error) if pending.notify => state.errors.push(error),
                Err(_) => {}
            }
        }
        if state.committed >= state.visible {
            state.dirty_since = None;
        }
        std::mem::take(&mut state.errors)
    }
    pub(crate) fn retry_latest_before_shutdown(&self) {
        let mut state = self.lock();
        if std::mem::replace(&mut state.retried, true) {
            return;
        }
        if state.visible > state.committed
            && !state.pending.iter().any(|p| p.revision == state.visible)
        {
            self.enqueue(&mut state, Ticket::default());
        }
    }
    pub(crate) fn status(&self) -> (u64, u64, usize) {
        let state = self.lock();
        (state.visible, state.committed, state.pending.len())
    }
    /// Continuous semantic dirty age. Reading this does not collect or consume any receipt.
    pub(crate) fn dirty_age_us(&self) -> Option<u64> {
        self.lock().dirty_since.map(|since| {
            u64::try_from(
                self.clock
                    .now()
                    .saturating_duration_since(since)
                    .as_micros(),
            )
            .unwrap_or(u64::MAX)
        })
    }
    pub(crate) fn finish_shutdown(&self) -> Option<(u64, u64)> {
        let mut state = self.lock();
        if std::mem::replace(&mut state.reported, true) {
            return None;
        }
        (state.visible > state.committed).then_some((state.visible, state.committed))
    }
}
