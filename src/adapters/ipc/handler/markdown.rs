//! `markdown.*` IPC 메서드 — 제자리 이동(in-place navigation, 04).
//!
//! `markdown.navigate`: 주어진 surface 를 **그 자리에서** 다른 파일의 markdown 으로
//! 교체한다(새 탭 아님). 주소창(03) 플러그인이 자기 surface_id + 새 경로로 호출한다.
//! 확장자 무관하게 markdown 으로 연다.
//!
//! 교체는 `ConvertSurface`(kind="markdown", params={file}) 재사용 — 같은 surface_id.
//! egui-mesh re-bootstrap(stale frame drop)은 `SurfaceConverted` cascade 가 처리한다.
//! 대용량 파일 확인은 **plugin 소유**다: 변환으로 새 `surface.create` 가 plugin 에
//! 전달되면 plugin 이 in-process 로 크기를 감지해 확인 팝업을 띄운다. host 는 파일
//! 크기를 stat 하지 않으므로 navigate 는 크기게이트 없이 즉시 변환만 한다.
//!
//! 최근목록 조회는 generic `recent.query {kind}`(handler/recent.rs)로 이관됐다.

use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

#[derive(Deserialize)]
struct NavigateReq {
    /// 교체 대상 surface (플러그인 자신의 surface).
    surface_id: u32,
    /// 이동할 파일의 절대 경로.
    path: String,
}

/// `markdown.navigate { surface_id, path }`. 크기게이트 없이 즉시 제자리 변환한다
/// (대용량 확인은 변환 후 plugin 이 소유).
pub fn handle_navigate(
    state: &mut AppState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: NavigateReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    let path = std::path::PathBuf::from(&req.path);
    if !path.exists() {
        return JsonRpcResponse::error(id, -32602, format!("path not found: {}", req.path));
    }

    navigate_now(state, req.surface_id, &req.path);
    JsonRpcResponse::success(id, json!({ "accepted": true }))
}

/// 같은 surface 를 markdown + 새 file 로 제자리 변환한다.
pub(crate) fn navigate_now(state: &mut AppState, surface_id: u32, path: &str) {
    state.dispatch_intent(
        crate::intent::Intent::ConvertSurface {
            surface_id,
            target: crate::intent::ConvertTarget::Kind {
                cwd: None,
                kind: "markdown".to_string(),
                params: json!({ "file": path }),
            },
        }
        .from_agent_ipc(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_req_parses_surface_and_path() {
        let req: NavigateReq =
            serde_json::from_value(json!({ "surface_id": 3, "path": "/a.md" })).unwrap();
        assert_eq!(req.surface_id, 3);
        assert_eq!(req.path, "/a.md");
    }
}
