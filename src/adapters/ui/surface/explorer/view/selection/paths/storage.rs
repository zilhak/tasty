//! Bulk UTF8 paths share an owned buffer; other paths keep their original OS bytes.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::fs_list::DirEntryInfo;

#[derive(Clone, Default)]
pub(super) struct PathStorage {
    pub(super) packed: String,
    pub(super) entries: Option<Arc<Vec<DirEntryInfo>>>,
}

#[derive(Clone, Debug)]
pub(super) enum StoredPath {
    Owned(PathBuf),
    Packed(Range<usize>),
    Entry(usize),
}

impl StoredPath {
    pub(super) fn empty() -> Self {
        Self::Owned(PathBuf::new())
    }

    pub(super) fn as_path<'a>(&'a self, storage: &'a PathStorage) -> &'a Path {
        match self {
            Self::Owned(path) => path,
            Self::Packed(range) => Path::new(&storage.packed[range.clone()]),
            Self::Entry(index) => &storage.entries.as_ref().expect("entry source")[*index].path,
        }
    }

    pub(super) fn into_owned(self, storage: &PathStorage) -> PathBuf {
        match self {
            Self::Owned(path) => path,
            Self::Packed(range) => Path::new(&storage.packed[range]).to_path_buf(),
            Self::Entry(index) => storage.entries.as_ref().expect("entry source")[index]
                .path
                .clone(),
        }
    }
}

pub(super) enum Incoming<'a> {
    Owned(PathBuf),
    Borrowed(&'a Path),
    Stored(StoredPath),
}

impl Incoming<'_> {
    pub(super) fn as_path<'a>(&'a self, storage: &'a PathStorage) -> &'a Path {
        match self {
            Self::Owned(path) => path,
            Self::Borrowed(path) => path,
            Self::Stored(path) => path.as_path(storage),
        }
    }

    pub(super) fn into_owned(self, storage: &PathStorage) -> PathBuf {
        match self {
            Self::Owned(path) => path,
            Self::Borrowed(path) => path.to_path_buf(),
            Self::Stored(path) => path.into_owned(storage),
        }
    }

    pub(super) fn store(self, storage: &mut PathStorage) -> StoredPath {
        match self {
            Self::Owned(path) => StoredPath::Owned(path),
            Self::Stored(path) => path,
            Self::Borrowed(path) => match path.to_str() {
                Some(text) => {
                    let start = storage.packed.len();
                    storage.packed.push_str(text);
                    StoredPath::Packed(start..storage.packed.len())
                }
                None => StoredPath::Owned(path.to_path_buf()),
            },
        }
    }
}
