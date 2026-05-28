//! FileSystem port — disk I/O 통합 인터페이스.
//!
//! `std::fs` 의 분산 함수들을 한 trait 으로. test 시 in-memory mock 으로 swap.

use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub trait FileSystem: Send + Sync {
    fn read_to_string(&self, path: &Path) -> anyhow::Result<String>;
    fn read_bytes(&self, path: &Path) -> anyhow::Result<Vec<u8>>;
    fn write_string(&self, path: &Path, content: &str) -> anyhow::Result<()>;
    fn write_bytes(&self, path: &Path, content: &[u8]) -> anyhow::Result<()>;
    fn create_dir_all(&self, path: &Path) -> anyhow::Result<()>;
    fn remove_file(&self, path: &Path) -> anyhow::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> anyhow::Result<()>;
    fn metadata(&self, path: &Path) -> anyhow::Result<FileMetadata>;
    fn exists(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>>;
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: std::time::SystemTime,
    pub is_dir: bool,
}
