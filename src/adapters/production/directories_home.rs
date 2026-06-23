//! DirectoriesHome — `directories` crate 기반 production HomeDirectory.

use std::path::PathBuf;

use crate::ports::home::HomeDirectory;

#[derive(Debug, Default)]
pub struct DirectoriesHome;

impl HomeDirectory for DirectoriesHome {
    fn home(&self) -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
    }

    fn tasty_config(&self) -> Option<PathBuf> {
        // 루트 진실원천(SoT)을 tasty_home() 하나로 단일화 — debug/release 격리
        // 및 TASTY_HOME override 가 포트 경유 경로에도 일관 적용된다.
        tasty_utils::path::tasty_home()
    }

    fn tasty_data(&self) -> Option<PathBuf> {
        // tasty 는 별 OS data dir 안 씀 — `~/.tasty/` 통일.
        self.tasty_config()
    }

    fn tasty_cache(&self) -> Option<PathBuf> {
        self.tasty_config().map(|c| c.join("cache"))
    }
}
