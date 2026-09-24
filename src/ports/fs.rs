//! 파일 I/O 인터페이스. 시험에서는 메모리 어댑터로 바꿀 수 있다.

use std::path::{Path, PathBuf};

#[allow(dead_code)] // 이유: 빌더와 시험용 구현은 있지만 제품 코드의 호출부는 없다
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

#[allow(dead_code)] // 이유: 빌더와 시험용 구현은 있지만 제품 코드의 호출부는 없다
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: std::time::SystemTime,
    pub is_dir: bool,
}
