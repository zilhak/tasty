//! Surface 도메인 Intent 핸들러.
//!
//! 정책:
//! - **SplitSurface**: `DomainIntent::SplitSurface` forward. focused
//!   surface_id 는 handler 안에서 결정. cascade 가 origin 보고 focus 이동.
//! - **CloseSurface**: origin.is_user() 면 snapshot 푸시 (Undo 가능),
//!   Agent 면 no_snapshot. C1=B / C2 결정.
//! - **ConvertSurface**: target 분기.
//!   - `Terminal` → `state.convert_surface_to_terminal(engine, sid)`.
//!   - `Kind { kind, params }` → 빌트인 wrapper (markdown/image/html) 가 있으면 그쪽,
//!     없으면 generic `convert_surface_to_kind` 호출.

use super::{ConvertTarget, DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::engine_state::CoreState;
use crate::model::SplitDirection;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    match &intent.body {
        Intent::SplitSurface { direction } => {
            split(core, state, engine, *direction, &intent.origin)
        }
        Intent::CloseSurface { surface_id } => close(state, engine, intent, *surface_id),
        Intent::ConvertSurface { surface_id, target } => {
            convert(state, engine, *surface_id, target)
        }
        _ => {}
    }
}

fn split(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    direction: SplitDirection,
    origin: &IntentOrigin,
) {
    let Some(sid) = state.focused_surface_id(engine) else {
        tracing::warn!("SplitSurface: no focused surface");
        return;
    };
    let cwd = state.resolve_inherit_cwd_from_surface(engine, sid);
    let intent = crate::core::intent::DomainIntent::SplitSurface {
        target_surface_id: sid,
        direction,
        cwd,
        kind: "terminal".to_string(),
        surface_params: serde_json::json!({}),
    };
    let events = match core.apply(engine, intent) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("SplitSurface failed: {e}");
            return;
        }
    };
    for ev in events {
        if let crate::core::intent::CoreEvent::SurfaceSplit {
            workspace_index,
            pane_id,
            new_surface_id,
            ..
        } = ev
        {
            crate::app::dispatch_domain::cascade_surface_split(
                engine,
                origin,
                workspace_index,
                pane_id,
                new_surface_id,
            );
        }
    }
}

fn close(state: &mut AppState, engine: &mut CoreState, intent: &DispatchedIntent, surface_id: u32) {
    // C1=B / C2: 사용자 동작만 closed-tab restore stack (snapshot) 에 push.
    if intent.origin.is_user() {
        state.close_surface_by_id(engine, surface_id);
    } else {
        state.close_surface_by_id_no_snapshot(engine, surface_id);
    }
}

fn convert(state: &mut AppState, engine: &mut CoreState, surface_id: u32, target: &ConvertTarget) {
    match target {
        ConvertTarget::Terminal => {
            state.convert_surface_to_terminal(engine, surface_id);
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
                    state.convert_surface_to_markdown(engine, surface_id, file_path);
                }
                "image" => {
                    state.convert_surface_to_image(engine, surface_id);
                }
                _ => {
                    state.convert_surface_to_kind(engine, surface_id, kind, params);
                }
            }
        }
    }
}
