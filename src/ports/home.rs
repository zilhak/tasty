//! HomeDirectory port — `~/.tasty/` 등 사용자 디렉토리 lookup.
//!
//! Test 시 tempdir 기반 adapter.

use std::path::PathBuf;

#[allow(dead_code)]
pub trait HomeDirectory: Send + Sync {
    fn home(&self) -> Option<PathBuf>;
    /// `~/.tasty/` (config root).
    fn tasty_config(&self) -> Option<PathBuf>;
    /// OS data dir 또는 `~/.tasty/data`.
    fn tasty_data(&self) -> Option<PathBuf>;
    /// OS cache dir 또는 `~/.tasty/cache`.
    fn tasty_cache(&self) -> Option<PathBuf>;
}
