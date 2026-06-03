//! `file_handler.*` IPC 메서드 — reload + dispatch.
//!
//! - `file_handler.reload`: user TOML 재로드. host/plugin 영향 없음.
//!   Method call wrapper (`Core::reload_file_handlers`) 직접 호출.
//! - `file_handler.dispatch`: 임의 경로를 file_handler 시스템에 진입시킴.
//!   `DomainIntent::DispatchFile` 발화 — Core::apply 가 worker spawn,
//!   결과는 `AppEvent::IdentifyDone` 경로로 비동기 적용.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::json;

use crate::core::Core;
use crate::file::format::{DetectDepth, FileTarget};
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub fn handle_reload(
    core: &Core,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let outcome = core.reload_file_handlers(engine);
    JsonRpcResponse::success(
        id,
        json!({
            "path": outcome.path.display().to_string(),
            "exists": outcome.exists,
        }),
    )
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
///   기본 `"deep"`. 두 경우 모두 worker thread 경유 (통일된 경로) — 응답은
///   즉시 돌아오고 handler 실행은 `AppEvent::IdentifyDone` 경로로 진행.
pub fn handle_dispatch(
    state: &mut AppState,
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
    state.dispatch_intent(
        crate::core::intent::DomainIntent::DispatchFile { target, depth }.from_agent_ipc(),
    );
    JsonRpcResponse::success(
        id,
        json!({
            "accepted": true,
            "depth": req.depth,
        }),
    )
}
