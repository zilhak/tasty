//! `markdown.*` IPC 메서드 — 제자리 이동(in-place navigation, 04).
//!
//! `markdown.navigate`: 주어진 surface 를 **그 자리에서** 다른 파일의 markdown 으로
//! 교체한다(새 탭 아님). 주소창(03) 플러그인이 자기 surface_id + 새 경로로 호출한다.
//! 확장자 무관하게 markdown 으로 연다. 1MB 초과 & !bypass 면 `01` 크기 확인 팝업을
//! 먼저 띄우고, [열기] 확정 시에만 교체한다.
//!
//! 교체는 `ConvertSurface`(kind="markdown", params={file}) 재사용 — 같은 surface_id.
//! egui-mesh re-bootstrap(stale frame drop)은 `SurfaceConverted` cascade 가 처리한다.
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
    /// true 면 대용량 확인 팝업을 건너뛰고 즉시 교체(강제).
    #[serde(default)]
    ignore_size_limit: bool,
}

/// `markdown.navigate { surface_id, path, ignore_size_limit? }`.
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

    // 대용량 확인 게이트 (01 재사용). 초과 & !bypass 면 교체를 보류하고 확인 팝업.
    if !req.ignore_size_limit
        && let Some(size) = crate::file::dispatch::exceeds_md_size_limit(&path)
    {
        state.dialogs.pending_md_open = Some(crate::state::PendingMdOpen {
            path: req.path.clone(),
            size,
            result: None,
            kind: crate::state::PendingMdOpenKind::InPlace {
                surface_id: req.surface_id,
            },
        });
        state.dispatch_intent(
            crate::intent::UiIntent::OpenPopup {
                id: "markdown_size_confirm",
                mode: crate::intent::OpenPopupMode::WithScope(
                    crate::adapters::ui::popup::PopupScope::Surface(req.surface_id),
                ),
            }
            .from_agent_ipc(),
        );
        return JsonRpcResponse::success(id, json!({ "accepted": true, "pending_confirm": true }));
    }

    // 즉시 제자리 변환.
    navigate_now(state, req.surface_id, &req.path);
    JsonRpcResponse::success(id, json!({ "accepted": true, "pending_confirm": false }))
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
    fn navigate_req_defaults_bypass_false() {
        let req: NavigateReq =
            serde_json::from_value(json!({ "surface_id": 3, "path": "/a.md" })).unwrap();
        assert_eq!(req.surface_id, 3);
        assert!(!req.ignore_size_limit);
    }

    #[test]
    fn navigate_req_parses_bypass() {
        let req: NavigateReq = serde_json::from_value(
            json!({ "surface_id": 5, "path": "/b.md", "ignore_size_limit": true }),
        )
        .unwrap();
        assert!(req.ignore_size_limit);
    }
}
