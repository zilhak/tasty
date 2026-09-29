//! html surface 스크립트 허용의 debug 재현 명령.
//!
//! 허용은 release에서 사용자가 GUI로만 한다(ADR-0053). 이 파일의 명령은 허용 클릭과
//! 뒤로·앞으로·재로드·중지 조작을 재현하므로 debug 빌드에만 둔다.

use serde_json::{Value, json};

use crate::ipc::protocol::JsonRpcResponse;
use crate::plugin_bridge::remote_surface::RemoteSurface;
use crate::state::AppState;
use crate::webview::DebugHistoryAction;

fn find_html_surface<'a>(
    engine: &'a crate::core::CoreState,
    sid: u32,
    id: &Value,
) -> Result<&'a RemoteSurface, JsonRpcResponse> {
    let Some(surface) = engine.find_surface_by_id(sid) else {
        return Err(JsonRpcResponse::error(
            id.clone(),
            -32000,
            "surface_id not found",
        ));
    };
    match surface.as_any().downcast_ref::<RemoteSurface>() {
        Some(rs) if rs.kind_static == "html" => Ok(rs),
        _ => Err(JsonRpcResponse::error(
            id.clone(),
            -32000,
            "surface is not an html surface",
        )),
    }
}

/// `debug.html_script.allow` — 허용 클릭을 재현한다. 현재 문서를 기록하고 재로드를 요청한다.
pub(super) fn handle_allow(
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match super::require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let rs = match find_html_surface(engine, sid, &id) {
        Ok(rs) => rs,
        Err(e) => return e,
    };
    let result = rs.with_html_script(|st| {
        st.allow_current().map(|()| {
            (
                st.current().map(|d| d.url.clone()),
                st.current_detection().map(|d| d.as_str()),
            )
        })
    });
    match result {
        Ok((url, detection)) => JsonRpcResponse::success(
            id,
            json!({ "allowed": true, "url": url, "detection": detection }),
        ),
        Err(e) => {
            tracing::warn!("debug.html_script.allow: surface {sid}: {e}");
            JsonRpcResponse::error(id, -32000, e.to_string())
        }
    }
}

/// `debug.webview.history` — 뒤로·앞으로·재로드·중지를 재현한다. 다음 redraw에서 적용한다.
pub(super) fn handle_history(
    state: &mut AppState,
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match super::require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let action = match params.get("action").and_then(|v| v.as_str()) {
        Some("back") => DebugHistoryAction::Back,
        Some("forward") => DebugHistoryAction::Forward,
        Some("reload") => DebugHistoryAction::Reload,
        Some("stop") => DebugHistoryAction::Stop,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "'action' must be one of back, forward, reload, stop",
            );
        }
    };
    if let Err(e) = find_html_surface(engine, sid, &id) {
        return e;
    }
    state.debug_webview_history.push((sid, action));
    JsonRpcResponse::success(id, json!({ "queued": true }))
}
