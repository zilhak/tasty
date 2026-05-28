//! TmpHome — 명시한 base 경로 위에 tasty config / data / cache 구성.
//!
//! test 시 base = tempfile::TempDir 의 경로. 실제 디스크 사용 시 `MemFileSystem` 과
//! 함께 쓰면 in-memory 완성.

use std::path::PathBuf;

use crate::ports::home::HomeDirectory;

pub struct TmpHome {
    base: PathBuf,
}

impl TmpHome {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}

impl HomeDirectory for TmpHome {
    fn home(&self) -> Option<PathBuf> {
        Some(self.base.clone())
    }

    fn tasty_config(&self) -> Option<PathBuf> {
        Some(self.base.join(".tasty"))
    }

    fn tasty_data(&self) -> Option<PathBuf> {
        self.tasty_config()
    }

    fn tasty_cache(&self) -> Option<PathBuf> {
        self.tasty_config().map(|c| c.join("cache"))
    }
}
