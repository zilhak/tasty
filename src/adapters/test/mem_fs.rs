//! MemFileSystem — in-memory FileSystem. test 시 disk I/O 우회.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::ports::fs::{FileMetadata, FileSystem};

#[derive(Debug, Default)]
pub struct MemFileSystem {
    entries: Mutex<HashMap<PathBuf, Entry>>,
}

#[derive(Debug, Clone)]
struct Entry {
    kind: Kind,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
enum Kind {
    File(Vec<u8>),
    Dir,
}

impl MemFileSystem {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FileSystem for MemFileSystem {
    fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
        let bytes = self.read_bytes(path)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn read_bytes(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        let entries = self.entries.lock().expect("MemFileSystem poisoned");
        match entries.get(path) {
            Some(Entry {
                kind: Kind::File(b),
                ..
            }) => Ok(b.clone()),
            Some(Entry {
                kind: Kind::Dir, ..
            }) => Err(anyhow::anyhow!("{:?} is a directory", path)),
            None => Err(anyhow::anyhow!("{:?} not found", path)),
        }
    }

    fn write_string(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        self.write_bytes(path, content.as_bytes())
    }

    fn write_bytes(&self, path: &Path, content: &[u8]) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().expect("MemFileSystem poisoned");
        entries.insert(
            path.to_path_buf(),
            Entry {
                kind: Kind::File(content.to_vec()),
                modified: SystemTime::now(),
            },
        );
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().expect("MemFileSystem poisoned");
        let mut cur = PathBuf::new();
        for comp in path.components() {
            cur.push(comp);
            entries.entry(cur.clone()).or_insert_with(|| Entry {
                kind: Kind::Dir,
                modified: SystemTime::now(),
            });
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().expect("MemFileSystem poisoned");
        entries
            .remove(path)
            .ok_or_else(|| anyhow::anyhow!("{:?} not found", path))?;
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().expect("MemFileSystem poisoned");
        let entry = entries
            .remove(from)
            .ok_or_else(|| anyhow::anyhow!("{:?} not found", from))?;
        entries.insert(to.to_path_buf(), entry);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> anyhow::Result<FileMetadata> {
        let entries = self.entries.lock().expect("MemFileSystem poisoned");
        let entry = entries
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("{:?} not found", path))?;
        let (size, is_dir) = match &entry.kind {
            Kind::File(b) => (b.len() as u64, false),
            Kind::Dir => (0, true),
        };
        Ok(FileMetadata {
            size,
            modified: entry.modified,
            is_dir,
        })
    }

    fn exists(&self, path: &Path) -> bool {
        let entries = self.entries.lock().expect("MemFileSystem poisoned");
        entries.contains_key(path)
    }

    fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let entries = self.entries.lock().expect("MemFileSystem poisoned");
        let dir_marker = path.to_path_buf();
        if !matches!(
            entries.get(&dir_marker),
            Some(Entry {
                kind: Kind::Dir,
                ..
            })
        ) && path != Path::new("")
        {
            // path 가 디렉토리 아니면 실패
            return Err(anyhow::anyhow!("{:?} is not a directory", path));
        }
        let mut result = Vec::new();
        for key in entries.keys() {
            if key.parent() == Some(path) {
                result.push(key.clone());
            }
        }
        result.sort();
        Ok(result)
    }
}
