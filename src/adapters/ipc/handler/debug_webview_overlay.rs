//! native WebView 오버레이 모드의 debug 조회·강제 명령.
//!
//! backend 가 보고하는 지원 모드를 덮어써 적용 경로를 재현하므로 debug 빌드에만 둔다. 지원하지 않는
//! backend 에 `opaque` 를 강제하면 적용이 실패해 그 프레임부터 숨김으로 돌아가고 경고가 남는다
//! (docs/design/systems/webview.md).

use serde_json::{Value, json};

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::RequestContext;
use crate::state::webview_overlay::{OverlayMode, TargetKind};

fn mode_name(mode: OverlayMode) -> &'static str {
    match mode {
        OverlayMode::Hide => "hide",
        OverlayMode::Opaque => "opaque",
        OverlayMode::Transparent => "transparent",
    }
}

fn kind_name(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::HostPopup => "host_popup",
        TargetKind::PluginPopup => "plugin_popup",
        TargetKind::Dropdown => "dropdown",
    }
}

/// `debug.webview.overlay` — 마지막 동기화의 모드·대상을 돌려준다. `force` 가 있으면 다음 redraw 부터
/// backend 지원 모드를 덮어쓴다(`off` 는 덮어쓰기 해제).
pub(super) fn handle_overlay(
    state: &mut RequestContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    match params.get("force").and_then(|v| v.as_str()) {
        None => {}
        Some(force) => {
            let forced = match force {
                "off" => None,
                "hide" => Some(OverlayMode::Hide),
                "opaque" => Some(OverlayMode::Opaque),
                "transparent" => Some(OverlayMode::Transparent),
                _ => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        "'force' must be one of off, hide, opaque, transparent",
                    );
                }
            };
            // 응답 뒤 라우터가 이 창을 dirty 로 표시하므로 다음 redraw 에서 반영된다.
            state.webview_overlay.forced = forced;
        }
    }
    let ov = &state.webview_overlay;
    let rect = |r: egui::Rect| json!([r.min.x, r.min.y, r.width(), r.height()]);
    JsonRpcResponse::success(
        id,
        json!({
            "mode": mode_name(ov.mode),
            "forced": ov.forced.map(mode_name),
            "applied": ov.applied,
            "failed": ov.health.failed(),
            "targets": ov.targets.iter().map(|t| json!({
                "kind": kind_name(t.kind),
                "rect": rect(t.rect),
                "scrim": t.scrim.map(rect),
            })).collect::<Vec<_>>(),
            "pages": ov.pages.iter().map(|r| rect(*r)).collect::<Vec<_>>(),
            "planned_pages": ov.planned_pages.iter().map(|r| rect(*r)).collect::<Vec<_>>(),
            "painted": ov.painted.as_ref().map(|p| json!({
                "mode": mode_name(p.mode),
                "pages": p.pages.iter().map(|r| rect(*r)).collect::<Vec<_>>(),
            })),
        }),
    )
}
