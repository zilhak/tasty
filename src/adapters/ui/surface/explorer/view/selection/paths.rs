//! A directory has unique names: hash that borrowed suffix and compare whole paths.
//! Equal names from recursive results share a normal path set, keeping lookup bounded
//! without rehashing every long parent prefix for the common single-directory case.

mod storage;

use crate::core::fs_list::DirEntryInfo;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use storage::{Incoming, PathStorage, StoredPath};

#[derive(Clone, Debug)]
enum Bucket {
    One(StoredPath),
    Many(std::sync::Arc<HashSet<PathBuf>>),
}

/// An unordered path set with the same equality as `HashSet<PathBuf>`.
#[derive(Clone, Default)]
pub struct SelectedPaths {
    buckets: Arc<HashMap<u64, Bucket, egui::ahash::RandomState>>,
    len: usize,
    storage: PathStorage,
    staged: Vec<StoredPath>,
}

impl SelectedPaths {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buckets: Arc::new(HashMap::with_capacity_and_hasher(
                capacity,
                Default::default(),
            )),
            len: 0,
            storage: PathStorage::default(),
            staged: Vec::new(),
        }
    }

    fn key(&self, path: &Path) -> u64 {
        // Path equality preserves file_name, including normalized separators and dots.
        // Roots and paths ending in '..' have no file name and use the full Path hash.
        path.file_name().map_or_else(
            || self.buckets.hasher().hash_one(path),
            |name| self.buckets.hasher().hash_one(name),
        )
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        if let Some(buckets) = Arc::get_mut(&mut self.buckets) {
            buckets.clear();
        } else {
            self.buckets = Arc::new(HashMap::with_hasher(self.buckets.hasher().clone()));
        }
        self.staged.clear();
        self.storage.packed.clear();
        self.storage.entries = None;
        self.len = 0;
    }

    pub fn contains(&self, path: &Path) -> bool {
        match self.buckets.get(&self.key(path)) {
            Some(Bucket::One(value)) => value.as_path(&self.storage) == path,
            Some(Bucket::Many(values)) => values.contains(path),
            None => false,
        }
    }

    pub fn insert(&mut self, path: PathBuf) -> bool {
        self.insert_value(Incoming::Owned(path))
    }

    fn insert_value(&mut self, path: Incoming<'_>) -> bool {
        let key = self.key(path.as_path(&self.storage));
        let inserted = match Arc::make_mut(&mut self.buckets).entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(Bucket::One(path.store(&mut self.storage)));
                true
            }
            Entry::Occupied(mut entry) => match entry.get_mut() {
                Bucket::One(value)
                    if value.as_path(&self.storage) == path.as_path(&self.storage) =>
                {
                    false
                }
                bucket @ Bucket::One(_) => {
                    let Bucket::One(previous) =
                        std::mem::replace(bucket, Bucket::One(StoredPath::empty()))
                    else {
                        unreachable!()
                    };
                    let values = HashSet::from([
                        previous.into_owned(&self.storage),
                        path.into_owned(&self.storage),
                    ]);
                    *bucket = Bucket::Many(std::sync::Arc::new(values));
                    true
                }
                Bucket::Many(values) => {
                    std::sync::Arc::make_mut(values).insert(path.into_owned(&self.storage))
                }
            },
        };
        self.len += usize::from(inserted);
        inserted
    }

    #[cfg(test)]
    pub(super) fn refill<'a>(&mut self, count: usize, paths: impl Iterator<Item = &'a Path>) {
        self.refill_observed(count, paths, |_| {});
    }

    pub(super) fn refill_observed<'a>(
        &mut self,
        count: usize,
        paths: impl Iterator<Item = &'a Path>,
        after_pack: impl FnOnce(usize),
    ) {
        self.clear();
        Arc::make_mut(&mut self.buckets).reserve(count);
        let mut paths = paths.peekable();
        let bytes = paths
            .peek()
            .and_then(|path| path.to_str())
            .map_or(0, str::len);
        self.storage.packed.reserve(bytes.saturating_mul(count));
        let mut staged = std::mem::take(&mut self.staged);
        staged.reserve(count);
        // Copy sequential bytes before random table writes; keep both allocations for reuse.
        for path in paths {
            staged.push(Incoming::Borrowed(path).store(&mut self.storage));
        }
        after_pack(self.storage.packed.len());
        for path in staged.drain(..) {
            self.insert_value(Incoming::Stored(path));
        }
        self.staged = staged;
    }

    /// The unfiltered listing already owns every selected path. Share that immutable
    /// snapshot and store entry indices, avoiding an allocation or byte copy per path.
    pub(in super::super) fn refill_entries(&mut self, entries: Arc<Vec<DirEntryInfo>>) {
        self.clear();
        let count = entries.len();
        Arc::make_mut(&mut self.buckets).reserve(count);
        self.storage.entries = Some(entries);
        for index in 0..count {
            self.insert_value(Incoming::Stored(StoredPath::Entry(index)));
        }
    }

    pub(super) fn packed_bytes(&self) -> usize {
        self.storage.packed.len()
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let key = self.key(path);
        let Entry::Occupied(mut entry) = Arc::make_mut(&mut self.buckets).entry(key) else {
            return false;
        };
        let (removed, empty) = match entry.get_mut() {
            Bucket::One(value) => {
                let matches = value.as_path(&self.storage) == path;
                (matches, matches)
            }
            Bucket::Many(values) => (
                std::sync::Arc::make_mut(values).remove(path),
                values.is_empty(),
            ),
        };
        if empty {
            entry.remove();
        }
        self.len -= usize::from(removed);
        removed
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        let mut len = 0;
        Arc::make_mut(&mut self.buckets).retain(|_, bucket| match bucket {
            Bucket::One(path) => {
                let retained = keep(path.as_path(&self.storage));
                len += usize::from(retained);
                retained
            }
            Bucket::Many(values) => {
                std::sync::Arc::make_mut(values).retain(|path| keep(path.as_path()));
                len += values.len();
                !values.is_empty()
            }
        });
        self.len = len;
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Path> {
        let storage = &self.storage;
        SizedIter {
            inner: self.buckets.values().flat_map(move |bucket| match bucket {
                Bucket::One(path) => BucketIter::One(Some(path.as_path(storage))),
                Bucket::Many(values) => BucketIter::Many(values.iter()),
            }),
            remaining: self.len,
        }
    }

    pub fn owned(&self) -> impl ExactSizeIterator<Item = PathBuf> {
        self.iter().map(Path::to_path_buf)
    }
}

impl std::fmt::Debug for SelectedPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

struct SizedIter<I> {
    inner: I,
    remaining: usize,
}

impl<I: Iterator> Iterator for SizedIter<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next();
        self.remaining -= usize::from(item.is_some());
        item
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<I: Iterator> ExactSizeIterator for SizedIter<I> {}

enum BucketIter<'a> {
    One(Option<&'a Path>),
    Many(std::collections::hash_set::Iter<'a, PathBuf>),
}

impl<'a> Iterator for BucketIter<'a> {
    type Item = &'a Path;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::One(path) => path.take(),
            Self::Many(paths) => paths.next().map(PathBuf::as_path),
        }
    }
}

impl Extend<PathBuf> for SelectedPaths {
    fn extend<T: IntoIterator<Item = PathBuf>>(&mut self, paths: T) {
        for path in paths {
            self.insert(path);
        }
    }
}

impl FromIterator<PathBuf> for SelectedPaths {
    fn from_iter<T: IntoIterator<Item = PathBuf>>(paths: T) -> Self {
        let paths = paths.into_iter();
        let mut set = Self::with_capacity(paths.size_hint().0);
        set.extend(paths);
        set
    }
}

impl<const N: usize> From<[PathBuf; N]> for SelectedPaths {
    fn from(paths: [PathBuf; N]) -> Self {
        paths.into_iter().collect()
    }
}

impl PartialEq for SelectedPaths {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().all(|path| other.contains(path))
    }
}
impl Eq for SelectedPaths {}

impl PartialEq<HashSet<PathBuf>> for SelectedPaths {
    fn eq(&self, other: &HashSet<PathBuf>) -> bool {
        self.len == other.len() && self.iter().all(|path| other.contains(path))
    }
}

#[cfg(test)]
mod tests;
