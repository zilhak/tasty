//! Immutable listing ownership shared with a selection without copying path bytes.

use std::ops::Deref;
use std::sync::Arc;

use crate::core::fs_list::DirEntryInfo;

#[derive(Clone, Default)]
pub(crate) struct EntrySnapshot(Arc<Vec<DirEntryInfo>>);

impl EntrySnapshot {
    pub(super) fn share(&self) -> Arc<Vec<DirEntryInfo>> {
        self.0.clone()
    }

    /// Hidden-file repartitioning consumes an unshared listing; a live selection
    /// keeps its old immutable snapshot when a copy is necessary.
    pub(super) fn into_vec(self) -> Vec<DirEntryInfo> {
        Arc::unwrap_or_clone(self.0)
    }
}

impl Deref for EntrySnapshot {
    type Target = Vec<DirEntryInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<DirEntryInfo>> for EntrySnapshot {
    fn from(entries: Vec<DirEntryInfo>) -> Self {
        Self(Arc::new(entries))
    }
}

impl FromIterator<DirEntryInfo> for EntrySnapshot {
    fn from_iter<T: IntoIterator<Item = DirEntryInfo>>(entries: T) -> Self {
        entries.into_iter().collect::<Vec<_>>().into()
    }
}
