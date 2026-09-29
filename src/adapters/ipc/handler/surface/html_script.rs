//! `surface.html_script`: html surface의 스크립트 감지·허용·배너 상태 조회(ADR-0053).
//!
//! 읽기만 한다. 배너 표지를 갱신하는 `update_banner`가 아니라 `banner_phase`를 부르므로
//! 조회가 배너를 띄우거나 사용자 열람으로 기록되지 않는다.

use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_model::html_script::HtmlScriptState;

use super::require_surface_id;
use crate::plugin_bridge::remote_surface::RemoteSurface;

pub(crate) fn handle_html_script(
    engine: &crate::core::CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let Some(surface) = engine.find_surface_by_id(surface_id) else {
        return JsonRpcResponse::invalid_params(id, format!("Surface {surface_id} not found"));
    };
    let Some(rs) = surface
        .as_any()
        .downcast_ref::<RemoteSurface>()
        .filter(|rs| rs.kind_static == "html")
    else {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Surface {surface_id} is not an html surface"),
        );
    };
    let mut result = rs.with_html_script(|st| state_json(st));
    result["surface_id"] = json!(surface_id);
    JsonRpcResponse::success(id, result)
}

fn state_json(st: &HtmlScriptState) -> Value {
    let document = st.current().map(|doc| {
        json!({
            "url": doc.url,
            "detection": doc.scan.map(|s| s.detection.as_str()),
            "fingerprint": doc.scan.map(|s| s.fingerprint.to_hex()),
        })
    });
    let allowance = st.allowance().map(|a| {
        json!({
            "url": a.url,
            "fingerprint": a.fingerprint.to_hex(),
        })
    });
    let flags = st.banner();
    json!({
        "sandbox": st.sandbox(),
        "document": document,
        "allowance": allowance,
        "allowed": st.current_is_allowed(),
        "javascript": st.effective_js(),
        "loading": st.loading_before_commit(),
        "banner": {
            "phase": st.banner_phase().as_str(),
            "viewed": flags.viewed,
            "pending_view": flags.pending_view,
            "shown": flags.shown,
            "dismissed": flags.dismissed,
        },
    })
}

#[cfg(test)]
mod tests;
