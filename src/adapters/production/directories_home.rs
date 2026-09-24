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
        // debug/release 분리와 TASTY_HOME 설정을 같은 tasty_home 함수로 적용한다.
        tasty_utils::path::tasty_home()
    }

    fn tasty_data(&self) -> Option<PathBuf> {
        // 별도 OS 데이터 폴더 대신 Tasty 설정 폴더를 사용한다.
        self.tasty_config()
    }

    fn tasty_cache(&self) -> Option<PathBuf> {
        self.tasty_config().map(|c| c.join("cache"))
    }
}
