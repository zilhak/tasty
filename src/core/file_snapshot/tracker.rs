//! Disk revisions for registry exports; effective-registry recomputation has a separate owner.
use super::{Document, PendingSave, Snapshot, Submit};
use std::sync::{Arc, Mutex};

pub(crate) struct Tracker {
    key: &'static str,
    document: Arc<Document>,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    visible: u64,
    committed: u64,
    latest: Option<Vec<u8>>,
    sink: Option<Submit>,
    pending: Vec<(u64, Box<dyn PendingSave>)>,
    errors: Vec<String>,
    retried: bool,
    reported: bool,
}
impl Tracker {
    pub(crate) fn new(key: &'static str, document: Arc<Document>) -> Self {
        Self::loaded(key, document, 0)
    }
    pub(crate) fn loaded(key: &'static str, document: Arc<Document>, revision: u64) -> Self {
        Self {
            key,
            document,
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
        self.lock().sink = Some(sink);
    }
    pub(crate) fn update(&self, bytes: Result<Vec<u8>, String>) {
        let mut state = self.lock();
        state.visible += 1;
        match bytes {
            Ok(bytes) => {
                state.latest = Some(bytes);
                self.enqueue(&mut state);
            }
            Err(error) => {
                state.latest = None;
                state.errors.push(error);
            }
        }
    }
    fn enqueue(&self, state: &mut State) {
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
            Ok(receipt) => state.pending.push((state.visible, receipt)),
            Err(error) => state.errors.push(error),
        }
    }
    pub(crate) fn poll(&self) -> Vec<String> {
        let mut state = self.lock();
        let mut index = 0;
        while index < state.pending.len() {
            let Some(result) = state.pending[index].1.poll() else {
                index += 1;
                continue;
            };
            let (revision, _) = state.pending.swap_remove(index);
            match result {
                Ok(()) => state.committed = state.committed.max(revision),
                Err(error) => state.errors.push(error),
            }
        }
        std::mem::take(&mut state.errors)
    }
    pub(crate) fn retry_latest_before_shutdown(&self) {
        let mut state = self.lock();
        if std::mem::replace(&mut state.retried, true) {
            return;
        }
        if state.visible > state.committed
            && !state.pending.iter().any(|(r, _)| *r == state.visible)
        {
            self.enqueue(&mut state);
        }
    }
    pub(crate) fn status(&self) -> (u64, u64, usize) {
        let state = self.lock();
        (state.visible, state.committed, state.pending.len())
    }
    pub(crate) fn finish_shutdown(&self) -> Option<(u64, u64)> {
        let mut state = self.lock();
        if std::mem::replace(&mut state.reported, true) {
            return None;
        }
        (state.visible > state.committed).then_some((state.visible, state.committed))
    }
}
