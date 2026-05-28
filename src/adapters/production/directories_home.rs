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
        self.home().map(|h| h.join(".tasty"))
    }

    fn tasty_data(&self) -> Option<PathBuf> {
        // tasty 는 별 OS data dir 안 씀 — `~/.tasty/` 통일.
        self.tasty_config()
    }

    fn tasty_cache(&self) -> Option<PathBuf> {
        self.tasty_config().map(|c| c.join("cache"))
    }
}
