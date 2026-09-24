//! 사용자 파일 형식·처리기 설정을 다시 읽는다. picker의 GUI 결과 처리는 file::dispatch::picker_apply에 있다.

use std::path::PathBuf;

use crate::core::Core;
use crate::core::CoreState;

pub(crate) struct ReloadFileHandlersOutcome {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
    /// 이번 reload에서 적용하지 않은 사용자 항목.
    pub(crate) rejected: Vec<tasty_file_handler::RejectedUserHandler>,
}

impl Core {
    pub(crate) fn reload_file_handlers(&self, engine: &CoreState) -> ReloadFileHandlersOutcome {
        let path = user_config_path();
        engine.file_format.reload_user_config(&path);
        let rejected = engine.file_handler.reload_user_config(&path);
        let exists = path.exists();
        ReloadFileHandlersOutcome {
            path,
            exists,
            rejected,
        }
    }
}

/// Tasty 홈을 찾지 못하면 공용 임시 경로를 쓴다. 그 위치에 파일이 있으면 읽을 수 있다.
fn user_config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        // 이유: 홈이 없을 때 쓰는 공유 설정 경로이며 인스턴스별로 격리하지 않는다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}
