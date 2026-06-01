//! File 도메인의 Core method wrapper (D.3.C.G.3).
//!
//! `Core::apply` 시그니처 (engine 만 받음) 가 *AppState 를 mutate 할 수 없는*
//! 분기들을 본 모듈의 Method 로 흡수한다. `apply_identify_result` /
//! `apply_file_picker_result` 는 popup 오픈 (state.popups) 과 dialogs 슬롯
//! 정리 (state.dialogs.file_handler_picker) 가 필요해 DomainIntent 가 아닌
//! Method call 로 분류 (옵션 C, verify-g3 §2.1).
//!
//! `reload_file_handlers` 는 단순 응답형 Method.

use std::path::PathBuf;

use crate::core::Core;
use crate::engine_state::CoreState;

/// `Core::reload_file_handlers` 응답.
pub(crate) struct ReloadFileHandlersOutcome {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
}

impl Core {
    /// User TOML (`~/.tasty/file-handlers.toml`) 재로드. file_format / file_handler
    /// registry 모두 reload. 응답: `{ path, exists }`.
    pub(crate) fn reload_file_handlers(&self, engine: &CoreState) -> ReloadFileHandlersOutcome {
        let path = user_config_path();
        engine.file_format.reload_user_config(&path);
        engine.file_handler.reload_user_config(&path);
        let exists = path.exists();
        ReloadFileHandlersOutcome { path, exists }
    }
}

/// `~/.tasty/file-handlers.toml`. 홈 디렉토리 결정 실패 시 임시 경로 — 그 경우
/// `exists` 가 false 로 돌아오므로 caller 가 인지 가능.
fn user_config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}
