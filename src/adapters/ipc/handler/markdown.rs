//! 같은 surface를 새 파일의 markdown으로 바꾼다. 확장자는 제한하지 않는다.
//! 대용량 확인은 변환 후 플러그인이 처리하므로 호스트는 크기 확인 없이 변환한다.
//! SurfaceConverted 후속 처리가 기존 mesh 프레임을 제거한다.

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

#[derive(Deserialize)]
struct NavigateReq {
    /// 교체 대상 surface (플러그인 자신의 surface).
    surface_id: u32,
    /// 이동할 파일의 절대 경로.
    path: String,
}

pub fn handle_navigate(
    out: &mut crate::ipc::window_port::IntentOutbox,
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

    navigate_now(out, req.surface_id, &req.path);
    JsonRpcResponse::success(id, json!({ "accepted": true }))
}

pub(crate) fn navigate_now(
    out: &mut crate::ipc::window_port::IntentOutbox,
    surface_id: u32,
    path: &str,
) {
    out.push(
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
