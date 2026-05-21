//! Surface 도메인 Intent 핸들러.
//!
//! 정책:
//! - **SplitSurface**: focused 의존 (사용자 단축키 전용). Intent 발화로 통일.
//!   origin 검사는 하지 않는다 — Agent 가 dispatch 해도 동작은 동일하지만 현재
//!   외부 표면 (IPC/CLI) 에서는 발화되지 않는다.
//! - **CloseSurface**: origin.is_user() 면 snapshot 푸시 (Undo 가능),
//!   Agent 면 no_snapshot. C1=B / C2 결정.
//! - **ConvertSurface**: target 분기.
//!   - `Terminal` → `state.convert_surface_to_terminal(sid)`.
//!   - `Kind { kind, params }` → 빌트인 wrapper (markdown/image/html) 가 있으면 그쪽,
//!     없으면 generic `convert_surface_to_kind` 호출.

use super::{ConvertTarget, DispatchedIntent, Intent};
use crate::model::SplitDirection;
use crate::state::AppState;

pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    match &intent.body {
        Intent::SplitSurface { direction } => split(state, *direction),
        Intent::CloseSurface { surface_id } => close(state, intent, *surface_id),
        Intent::ConvertSurface { surface_id, target } => convert(state, *surface_id, target),
        _ => {}
    }
}

fn split(state: &mut AppState, direction: SplitDirection) {
    if let Err(e) = state.split_surface(direction) {
        tracing::warn!("split_surface failed: {e}");
    }
}

fn close(state: &mut AppState, intent: &DispatchedIntent, surface_id: u32) {
    // C1=B / C2: 사용자 동작만 closed-tab restore stack (snapshot) 에 push.
    if intent.origin.is_user() {
        state.close_surface_by_id(surface_id);
    } else {
        state.close_surface_by_id_no_snapshot(surface_id);
    }
}

fn convert(state: &mut AppState, surface_id: u32, target: &ConvertTarget) {
    match target {
        ConvertTarget::Terminal => {
            state.convert_surface_to_terminal(surface_id);
        }
        ConvertTarget::Kind { kind, params } => {
            // 빌트인 wrapper: 파라미터 모양이 정해진 변환은 전용 메서드로.
            match kind.as_str() {
                "markdown" => {
                    let Some(file_path) = params
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                    else {
                        tracing::warn!(
                            "convert markdown: missing or invalid 'file_path' param: {params}"
                        );
                        return;
                    };
                    state.convert_surface_to_markdown(surface_id, file_path);
                }
                "image" => {
                    state.convert_surface_to_image(surface_id);
                }
                _ => {
                    state.convert_surface_to_kind(surface_id, kind, params);
                }
            }
        }
    }
}
