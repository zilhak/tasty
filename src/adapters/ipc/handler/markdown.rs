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
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: NavigateReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    // mirror surface의 경로는 원격 파일이므로 존재 여부를 원격 서버가 판단한다.
    if !engine.is_mirror_surface(req.surface_id) && !std::path::Path::new(&req.path).exists() {
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

    const MISSING: &str = "/definitely/not/on/this/machine.md";

    fn navigate(mirror: bool) -> (JsonRpcResponse, Vec<crate::intent::DispatchedIntent>, u32) {
        let (state, mut engine) = crate::state::tests::test_state();
        let sid = *state
            .active_workspace(&engine)
            .all_surface_ids()
            .first()
            .expect("fixture surface");
        engine.workspaces[0].mirror = mirror;
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let resp = handle_navigate(
            &mut out,
            &engine,
            json!(1),
            json!({ "surface_id": sid, "path": MISSING }),
        );
        (resp, out.into_vec(), sid)
    }

    /// 원격 경로는 attach한 로컬 컴퓨터에 없을 수 있으므로 원격 서버에 판단을 맡긴다.
    #[test]
    fn navigate_on_mirror_surface_skips_local_exists_check() {
        let (resp, emitted, sid) = navigate(true);
        assert!(resp.error.is_none(), "navigate: {:?}", resp.error);
        assert_eq!(emitted.len(), 1);
        let crate::intent::Intent::ConvertSurface {
            surface_id,
            target: crate::intent::ConvertTarget::Kind { kind, params, .. },
        } = &emitted[0].body
        else {
            panic!("markdown.navigate 가 ConvertSurface 외 intent 를 냈다");
        };
        assert_eq!(*surface_id, sid);
        assert_eq!(kind, "markdown");
        assert_eq!(params["file"], MISSING);
    }

    #[test]
    fn navigate_on_local_surface_still_rejects_missing_path() {
        let (resp, emitted, _) = navigate(false);
        let err = resp.error.expect("로컬 surface 의 없는 경로는 거절한다");
        assert!(err.message.contains("path not found"), "{}", err.message);
        assert!(emitted.is_empty());
    }
}
