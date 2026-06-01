//! Tab 도메인 Intent 핸들러.
//!
//! 정책:
//! - **NewTab**: `DomainIntent::CreateTab` 으로 forward. focused pane 의 id 는
//!   handler 안에서 결정 (`state.active_workspace(engine).focused_pane`).
//!   terminal kind 면 cwd 도 handler 가 inherit 결정.
//! - **CloseTab**: ID 지정. close_tab_by_tab_id 호출. fire-and-forget.
//!   (B.5.2 에서 DomainIntent 로 마이그레이션 예정.)

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::engine_state::CoreState;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    match &intent.body {
        Intent::NewTab { kind, params } => new_tab(core, state, engine, kind.as_deref(), params),
        Intent::CloseTab { tab_id } => close_tab(state, engine, *tab_id),
        _ => {}
    }
}

fn new_tab(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    kind: Option<&str>,
    params: &serde_json::Value,
) {
    let kind = kind.unwrap_or("terminal");
    let pane_id = state.active_workspace(engine).focused_pane;
    let cwd = if kind == "terminal" {
        state.resolve_inherit_cwd(engine)
    } else {
        None
    };
    let surface_params = if params.is_null() {
        serde_json::json!({})
    } else {
        params.clone()
    };

    let intent = crate::core::intent::DomainIntent::CreateTab {
        pane_id,
        cwd,
        kind: kind.to_string(),
        surface_params,
    };
    if let Err(e) = core.apply(engine, intent) {
        tracing::warn!("NewTab kind={kind} failed: {e}");
    }
}

fn close_tab(state: &mut AppState, engine: &mut CoreState, tab_id: u32) {
    state.close_tab_by_tab_id(engine, tab_id);
}
