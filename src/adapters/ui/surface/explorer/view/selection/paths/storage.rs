//! Bulk UTF8 paths share an owned buffer; other paths keep their original OS bytes.

use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(super) enum StoredPath {
    Owned(PathBuf),
    Packed(Range<usize>),
}

impl StoredPath {
    pub(super) fn empty() -> Self {
        Self::Owned(PathBuf::new())
    }

    pub(super) fn as_path<'a>(&'a self, packed: &'a str) -> &'a Path {
        match self {
            Self::Owned(path) => path,
            Self::Packed(range) => Path::new(&packed[range.clone()]),
        }
    }

    pub(super) fn into_owned(self, packed: &str) -> PathBuf {
        match self {
            Self::Owned(path) => path,
            Self::Packed(range) => Path::new(&packed[range]).to_path_buf(),
        }
    }
}

pub(super) enum Incoming<'a> {
    Owned(PathBuf),
    Borrowed(&'a Path),
}

impl Incoming<'_> {
    pub(super) fn as_path(&self) -> &Path {
        match self {
            Self::Owned(path) => path,
            Self::Borrowed(path) => path,
        }
    }

    pub(super) fn into_owned(self) -> PathBuf {
        match self {
            Self::Owned(path) => path,
            Self::Borrowed(path) => path.to_path_buf(),
        }
    }

    pub(super) fn store(self, packed: &mut String) -> StoredPath {
        match self {
            Self::Owned(path) => StoredPath::Owned(path),
            Self::Borrowed(path) => match path.to_str() {
                Some(text) => {
                    let start = packed.len();
                    packed.push_str(text);
                    StoredPath::Packed(start..packed.len())
                }
                None => StoredPath::Owned(path.to_path_buf()),
            },
        }
    }
}
