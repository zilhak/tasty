//! StdFileSystem — `std::fs` 기반 production FileSystem.

use std::path::{Path, PathBuf};

use crate::ports::fs::{FileMetadata, FileSystem};

#[derive(Debug, Default)]
pub struct StdFileSystem;

impl FileSystem for StdFileSystem {
    fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn read_bytes(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        Ok(std::fs::read(path)?)
    }

    fn write_string(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        Ok(std::fs::write(path, content)?)
    }

    fn write_bytes(&self, path: &Path, content: &[u8]) -> anyhow::Result<()> {
        Ok(std::fs::write(path, content)?)
    }

    fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        Ok(std::fs::create_dir_all(path)?)
    }

    fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
        Ok(std::fs::remove_file(path)?)
    }

    fn rename(&self, from: &Path, to: &Path) -> anyhow::Result<()> {
        Ok(std::fs::rename(from, to)?)
    }

    fn metadata(&self, path: &Path) -> anyhow::Result<FileMetadata> {
        let meta = std::fs::metadata(path)?;
        Ok(FileMetadata {
            size: meta.len(),
            modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            is_dir: meta.is_dir(),
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|entry| entry.path()))
            .collect())
    }
}
