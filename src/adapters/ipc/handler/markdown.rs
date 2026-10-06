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
    /// 요청을 확정한 file-open 팝업. 호출 플러그인 소유이고 사용자 입력을 받았으면 사용자 요청이다.
    #[serde(default)]
    owner_popup_instance: Option<u64>,
}

pub fn handle_navigate(
    out: &mut crate::ipc::window_port::IntentOutbox,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    caller: &tasty_ipc::caller::CallerContext,
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

    // 원격 변환 실패를 toast로 알릴지도 이 출처로 정한다.
    let origin = super::file_handler::dispatch_origin_of(
        window,
        engine,
        caller,
        req.owner_popup_instance,
        None,
        None,
    );
    let intent = crate::intent::Intent::ConvertSurface {
        surface_id: req.surface_id,
        target: crate::intent::ConvertTarget::Kind {
            cwd: None,
            kind: "markdown".to_string(),
            params: json!({ "file": req.path }),
        },
    };
    out.push(match origin {
        crate::file::dispatch::FileDispatchOrigin::User => intent.from_user_menu("plugin_popup"),
        crate::file::dispatch::FileDispatchOrigin::Agent
        | crate::file::dispatch::FileDispatchOrigin::PluginUnverified => intent.from_agent_ipc(),
    });
    JsonRpcResponse::success(id, json!({ "accepted": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_ipc::caller::CallerContext;

    #[test]
    fn navigate_req_parses_surface_and_path() {
        let req: NavigateReq =
            serde_json::from_value(json!({ "surface_id": 3, "path": "/a.md" })).unwrap();
        assert_eq!(req.surface_id, 3);
        assert_eq!(req.path, "/a.md");
    }

    const MISSING: &str = "/definitely/not/on/this/machine.md";
    const PLUGIN: &str = "com.tasty.markdown";
    const POPUP: u64 = 7;

    fn plugin_caller(plugin_id: &str) -> CallerContext {
        CallerContext::Plugin {
            plugin_id: plugin_id.to_string(),
            permissions: std::sync::Arc::new(std::collections::HashSet::new()),
        }
    }

    /// activated는 확정 입력을 받은 팝업의 (소유 플러그인, 인스턴스)다.
    fn navigate_with(
        mirror: bool,
        caller: &CallerContext,
        activated: Option<(&str, u64)>,
        extra: serde_json::Value,
    ) -> (JsonRpcResponse, Vec<crate::intent::DispatchedIntent>, u32) {
        let (mut state, mut engine_session) = if mirror {
            crate::state::tests::test_mirror_state()
        } else {
            crate::state::tests::test_state()
        };
        let engine = engine_session.borrow_mut();
        let sid = *state
            .active_workspace(&engine)
            .all_surface_ids()
            .first()
            .expect("fixture surface");
        if let Some((plugin, instance)) = activated {
            state
                .plugin_popup_user_activated
                .insert(instance, plugin.to_string());
        }
        let mut params = json!({ "surface_id": sid, "path": MISSING });
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        let mut out = crate::ipc::window_port::IntentOutbox::default();
        let mut scope = crate::ipc::request_scope::RequestScope::capture(&mut state, &engine, None);
        let resp = handle_navigate(
            &mut out,
            &mut scope,
            &engine.as_ref(),
            caller,
            json!(1),
            params,
        );
        (resp, out.into_vec(), sid)
    }

    fn navigate(mirror: bool) -> (JsonRpcResponse, Vec<crate::intent::DispatchedIntent>, u32) {
        navigate_with(mirror, &CallerContext::local(), None, json!({}))
    }

    /// 대상이 mirror라 경로 검사 없이 intent가 나온다. 반환값은 사용자 요청 여부다.
    fn navigate_is_user(
        caller: &CallerContext,
        activated: Option<(&str, u64)>,
        extra: serde_json::Value,
    ) -> bool {
        let (resp, emitted, _) = navigate_with(true, caller, activated, extra);
        assert!(resp.error.is_none(), "navigate: {:?}", resp.error);
        assert_eq!(emitted.len(), 1);
        emitted[0].origin.is_user()
    }

    #[test]
    fn navigate_from_user_activated_own_popup_is_user_origin() {
        assert!(navigate_is_user(
            &plugin_caller(PLUGIN),
            Some((PLUGIN, POPUP)),
            json!({ "owner_popup_instance": POPUP }),
        ));
    }

    #[test]
    fn navigate_naming_another_plugins_popup_is_agent_origin() {
        assert!(!navigate_is_user(
            &plugin_caller(PLUGIN),
            Some(("com.example.other", POPUP)),
            json!({ "owner_popup_instance": POPUP }),
        ));
    }

    /// 닫혔거나 사용자 입력을 받지 않은 팝업은 활성 기록이 없다.
    #[test]
    fn navigate_naming_a_closed_popup_is_agent_origin() {
        assert!(!navigate_is_user(
            &plugin_caller(PLUGIN),
            None,
            json!({ "owner_popup_instance": POPUP }),
        ));
    }

    #[test]
    fn navigate_from_external_ipc_stays_agent_origin_even_with_owner_field() {
        assert!(!navigate_is_user(
            &CallerContext::local(),
            Some((PLUGIN, POPUP)),
            json!({ "owner_popup_instance": POPUP }),
        ));
    }

    /// 주소창 이동처럼 팝업 없이 보낸 플러그인 요청은 에이전트 요청이다.
    #[test]
    fn navigate_without_owner_popup_is_agent_origin() {
        assert!(!navigate_is_user(
            &plugin_caller(PLUGIN),
            Some((PLUGIN, POPUP)),
            json!({}),
        ));
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
