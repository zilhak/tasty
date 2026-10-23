//! Immutable listing ownership shared with a selection without copying path bytes.

use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;

use super::SelectedPaths;

use crate::core::fs_list::DirEntryInfo;

#[derive(Clone, Default)]
pub(crate) struct EntrySnapshot {
    entries: Arc<Vec<DirEntryInfo>>,
    selection: SelectedPaths,
}

impl EntrySnapshot {
    pub(super) fn selected_all(&self) -> SelectedPaths {
        self.selection.clone()
    }

    /// Hidden-file repartitioning consumes an unshared listing; a live selection
    /// keeps its old immutable snapshot when a copy is necessary.
    pub(super) fn into_vec(self) -> Vec<DirEntryInfo> {
        let Self { entries, selection } = self;
        drop(selection);
        Arc::unwrap_or_clone(entries)
    }
}

impl Deref for EntrySnapshot {
    type Target = Vec<DirEntryInfo>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl From<Vec<DirEntryInfo>> for EntrySnapshot {
    fn from(entries: Vec<DirEntryInfo>) -> Self {
        let started = tracing::enabled!(target: "tasty::explorer::perf", tracing::Level::TRACE)
            .then(Instant::now);
        let entries = Arc::new(entries);
        let mut selection = SelectedPaths::default();
        selection.refill_entries(entries.clone());
        if let Some(started) = started {
            tracing::trace!(target: "tasty::explorer::perf", entries = entries.len(), elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "selection_snapshot_prepared");
        }
        Self { entries, selection }
    }
}

impl FromIterator<DirEntryInfo> for EntrySnapshot {
    fn from_iter<T: IntoIterator<Item = DirEntryInfo>>(entries: T) -> Self {
        entries.into_iter().collect::<Vec<_>>().into()
    }
}
