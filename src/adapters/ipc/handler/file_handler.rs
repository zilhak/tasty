//! `file_handler.*` IPC 메서드 — reload + dispatch.
//!
//! - `file_handler.reload`: user TOML 재로드. host/plugin 영향 없음.
//! - `file_handler.dispatch`: 임의 경로를 file_handler 시스템에 진입시킴.
//!   explorer plugin 등에서 더블클릭 처리에 사용.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::json;

use crate::file::format::{DetectDepth, FileTarget};
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub fn handle_reload(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let path = user_config_path();
    engine.file_format.reload_user_config(&path);
    engine.file_handler.reload_user_config(&path);
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
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}

#[derive(Deserialize)]
struct DispatchReq {
    path: String,
    #[serde(default = "default_depth")]
    depth: String,
}

fn default_depth() -> String {
    "deep".to_string()
}

/// 임의 경로를 file_handler 디스패치 흐름에 진입시킨다. ctrl+click / drag&drop 과
/// 동일 흐름이지만 plugin / CLI 가 프로그래밍으로 호출하는 진입점.
///
/// - `params.path`: 절대 경로 권장. 상대 경로는 caller cwd 기준이라 비결정적.
/// - `params.depth`: `"cheap"` (확장자/glob 만) 또는 `"deep"` (magic/MIME 포함).
///   기본 `"deep"`. Deep 은 IdentifyWorker 로 비동기 — 응답은 즉시 돌아오고
///   handler 실행은 `AppEvent::IdentifyDone` 경로로 진행.
pub fn handle_dispatch(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: DispatchReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => {
            return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}"));
        }
    };
    let depth = match req.depth.as_str() {
        "cheap" => DetectDepth::Cheap,
        "deep" => DetectDepth::Deep,
        other => {
            return JsonRpcResponse::error(
                id,
                -32602,
                format!("invalid depth '{other}': expected 'cheap' or 'deep'"),
            );
        }
    };
    let target = FileTarget::new(PathBuf::from(&req.path));
    crate::file_dispatch::dispatch_file_target(state, engine, target, depth);
    JsonRpcResponse::success(
        id,
        json!({
            "accepted": true,
            "depth": req.depth,
        }),
    )
}
