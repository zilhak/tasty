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
    /// 결과 surface 를 이 surface 가 속한 *Pane* 에 새 tab 으로 추가한다.
    /// None 이면 기존 동작 (focused pane 의 새 탭).
    #[serde(default)]
    origin_surface_id: Option<u32>,
    /// true 면 대용량 markdown 확인 팝업을 건너뛰고 즉시 연다(에이전트 강제 열기).
    /// 기본 false — 1MB 초과 markdown 은 확인 팝업이 뜬다.
    #[serde(default)]
    ignore_size_limit: bool,
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
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: DispatchReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => {
            return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}"));
        }
    };
    if let Some(sid) = req.origin_surface_id
        && let Err(message) = crate::file::dispatch::require_origin_pane(engine, sid)
    {
        return JsonRpcResponse::invalid_params(id, message);
    }
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
    // 이 IPC 는 파일 경로만 받는다. URL 을 여는 것은 이 메서드의 권한 토큰(`FsRead`)이
    // 덮는 능력이 아니고, 경로 자리에 담긴 URL 은 확장자로 오식별된다 — 거절한다.
    if crate::file::format::looks_like_url(&req.path) {
        return JsonRpcResponse::error(
            id,
            -32602,
            format!(
                "invalid path '{}': file_handler.dispatch accepts file paths, not URLs",
                req.path
            ),
        );
    }
    let target = FileTarget::new(PathBuf::from(&req.path));
    state.dispatch_intent(
        crate::core::intent::DomainIntent::DispatchFile {
            target,
            depth,
            origin_surface_id: req.origin_surface_id,
            ignore_size_limit: req.ignore_size_limit,
        }
        .from_agent_ipc(),
    );
    JsonRpcResponse::success(
        id,
        json!({
            "accepted": true,
            "depth": req.depth,
            "ignore_size_limit": req.ignore_size_limit,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_size_limit_defaults_false() {
        let req: DispatchReq =
            serde_json::from_value(serde_json::json!({ "path": "/a.md" })).unwrap();
        assert!(!req.ignore_size_limit);
    }

    #[test]
    fn ignore_size_limit_parsed_true() {
        let req: DispatchReq = serde_json::from_value(
            serde_json::json!({ "path": "/a.md", "ignore_size_limit": true }),
        )
        .unwrap();
        assert!(req.ignore_size_limit);
    }

    /// 경로 자리에 URL 이 오면 dispatch 인텐트를 세우지 않고 거절한다 — 세우면
    /// `https://example.com/a.md` 가 확장자로 markdown 핸들러에 걸린다.
    #[test]
    fn dispatch_rejects_a_url_in_the_path_param() {
        let (mut state, engine) = crate::state::tests::test_state();
        let resp = handle_dispatch(
            &mut state,
            &engine,
            serde_json::json!(1),
            serde_json::json!({ "path": "https://example.com/a.md" }),
        );
        let err = resp.error.expect("URL path must be rejected");
        assert_eq!(err.code, -32602);
        assert!(state.pending_intents.is_empty());

        let resp = handle_dispatch(
            &mut state,
            &engine,
            serde_json::json!(2),
            serde_json::json!({ "path": "/tmp/a.md" }),
        );
        assert!(resp.error.is_none());
        assert_eq!(state.pending_intents.len(), 1);
    }
    #[test]
    fn dispatch_rejects_missing_origin_before_enqueueing() {
        let (mut state, engine) = crate::state::tests::test_state();
        let response = handle_dispatch(
            &mut state,
            &engine,
            serde_json::json!(42),
            serde_json::json!({"path":"/a", "origin_surface_id":u32::MAX}),
        );
        assert_eq!(response.error.unwrap().code, -32602);
        assert!(state.pending_intents.is_empty());
    }
}
