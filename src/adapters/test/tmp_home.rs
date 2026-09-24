//! 지정한 base 아래의 Tasty 설정·데이터·캐시 경로를 제공한다.
//! 디스크를 사용하는 시험은 TempDir 경로를 전달한다.

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
