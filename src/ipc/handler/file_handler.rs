//! `file_handler.*` IPC 메서드. 현재는 reload 한 가지.
//!
//! Phase A 에서 등록한 두 registry (`FileFormatRegistry`, `FileHandlerRegistry`)
//! 가 보관하는 user origin contribution 만 다시 읽는다. host/plugin 항목은 영향
//! 없음 — `reload_user_config` 가 transactional 로 user 만 swap.

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_reload(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let path = user_config_path();
    state.engine.file_format.reload_user_config(&path);
    state.engine.file_handler.reload_user_config(&path);
    let exists = path.exists();
    JsonRpcResponse::success(
        id,
        json!({
            "path": path.display().to_string(),
            "exists": exists,
        }),
    )
}

/// `~/.tasty/file-handlers.toml`. 홈 디렉토리 결정 실패 시 임시 경로 — 그 경우
/// `exists` 가 false 로 돌아오므로 caller 가 인지 가능.
fn user_config_path() -> std::path::PathBuf {
    tasty_core::paths::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}
