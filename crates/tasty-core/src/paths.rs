//! 경로 헬퍼. `~/.tasty/` 같은 공용 디렉토리 경로를 반환한다.

use std::path::PathBuf;

use directories::BaseDirs;

/// Returns the Tasty home directory: ~/.tasty/
/// Consistent across all platforms for easy AI/agent access.
pub fn tasty_home() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty"))
}
