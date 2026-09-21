//! File 도메인의 Core method wrapper.
//!
//! `reload_file_handlers` 는 단순 응답형 Method.
//!
//! identify 결과·picker 결과 적용(`apply_identify_result` / `apply_file_picker_result`)은
//! 여기 없다 — popup 을 열고 dialogs 슬롯을 만지는 GUI 동작이라
//! `crate::file::dispatch::picker_apply` 에 있다.

use std::path::PathBuf;

use crate::core::Core;
use crate::core::CoreState;

/// `Core::reload_file_handlers` 응답.
pub(crate) struct ReloadFileHandlersOutcome {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
    /// file handler registry 가 이번 reload 에서 적용하지 않은 user 항목.
    pub(crate) rejected: Vec<tasty_file_handler::RejectedUserHandler>,
}

impl Core {
    /// User TOML (`~/.tasty/file-handlers.toml`) 재로드. file_format / file_handler
    /// registry 모두 reload. 응답: `{ path, exists, rejected }`.
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

/// `~/.tasty/file-handlers.toml`. 홈 디렉토리 결정 실패 시 임시 경로 — 그 경우
/// `exists` 가 false 로 돌아오므로 caller 가 인지 가능.
fn user_config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        // 이유: 홈 미해결(CI 등)에서만 쓰는 공유 폴백. 인스턴스별 격리가 목적이 아니라
        // 사용자 config 라 의도된 공유다 — 이 경우 exists=false 라 caller 가 인지한다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}
