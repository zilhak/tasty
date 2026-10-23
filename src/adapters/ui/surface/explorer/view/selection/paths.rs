//! A directory has unique names: hash that borrowed suffix and compare whole paths.
//! Equal names from recursive results share a normal path set, keeping lookup bounded
//! without rehashing every long parent prefix for the common single-directory case.

mod storage;

use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::path::{Path, PathBuf};
use storage::{Incoming, StoredPath};

#[derive(Clone, Debug)]
enum Bucket {
    One(StoredPath),
    Many(std::sync::Arc<HashSet<PathBuf>>),
}

/// An unordered path set with the same equality as `HashSet<PathBuf>`.
#[derive(Clone, Default)]
pub struct SelectedPaths {
    buckets: HashMap<u64, Bucket, egui::ahash::RandomState>,
    len: usize,
    packed: String,
    staged: Vec<StoredPath>,
}

impl SelectedPaths {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buckets: HashMap::with_capacity_and_hasher(capacity, Default::default()),
            len: 0,
            packed: String::new(),
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
        self.buckets.clear();
        self.staged.clear();
        self.packed.clear();
        self.len = 0;
    }

    pub fn contains(&self, path: &Path) -> bool {
        match self.buckets.get(&self.key(path)) {
            Some(Bucket::One(value)) => value.as_path(&self.packed) == path,
            Some(Bucket::Many(values)) => values.contains(path),
            None => false,
        }
    }

    pub fn insert(&mut self, path: PathBuf) -> bool {
        self.insert_value(Incoming::Owned(path))
    }

    fn insert_value(&mut self, path: Incoming<'_>) -> bool {
        let inserted = match self.buckets.entry(self.key(path.as_path(&self.packed))) {
            Entry::Vacant(entry) => {
                entry.insert(Bucket::One(path.store(&mut self.packed)));
                true
            }
            Entry::Occupied(mut entry) => match entry.get_mut() {
                Bucket::One(value) if value.as_path(&self.packed) == path.as_path(&self.packed) => {
                    false
                }
                bucket @ Bucket::One(_) => {
                    let Bucket::One(previous) =
                        std::mem::replace(bucket, Bucket::One(StoredPath::empty()))
                    else {
                        unreachable!()
                    };
                    let values = HashSet::from([
                        previous.into_owned(&self.packed),
                        path.into_owned(&self.packed),
                    ]);
                    *bucket = Bucket::Many(std::sync::Arc::new(values));
                    true
                }
                Bucket::Many(values) => {
                    std::sync::Arc::make_mut(values).insert(path.into_owned(&self.packed))
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
        self.buckets.reserve(count);
        let mut paths = paths.peekable();
        let bytes = paths
            .peek()
            .and_then(|path| path.to_str())
            .map_or(0, str::len);
        self.packed.reserve(bytes.saturating_mul(count));
        let mut staged = std::mem::take(&mut self.staged);
        staged.reserve(count);
        // Copy sequential bytes before random table writes; keep both allocations for reuse.
        for path in paths {
            staged.push(Incoming::Borrowed(path).store(&mut self.packed));
        }
        after_pack(self.packed.len());
        for path in staged.drain(..) {
            self.insert_value(Incoming::Stored(path));
        }
        self.staged = staged;
    }

    pub(super) fn packed_bytes(&self) -> usize {
        self.packed.len()
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let Entry::Occupied(mut entry) = self.buckets.entry(self.key(path)) else {
            return false;
        };
        let (removed, empty) = match entry.get_mut() {
            Bucket::One(value) => {
                let matches = value.as_path(&self.packed) == path;
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
        self.buckets.retain(|_, bucket| match bucket {
            Bucket::One(path) => {
                let retained = keep(path.as_path(&self.packed));
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
        let packed = self.packed.as_str();
        SizedIter {
            inner: self.buckets.values().flat_map(move |bucket| match bucket {
                Bucket::One(path) => BucketIter::One(Some(path.as_path(packed))),
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
