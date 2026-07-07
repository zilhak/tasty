//! Tab 도메인 Intent 핸들러.
//!
//! 정책:
//! - **NewTab**: `DomainIntent::CreateTab` 으로 forward. focused pane 의 id 는
//!   handler 안에서 결정 (`state.active_workspace(engine).focused_pane`).
//!   terminal kind 면 cwd 도 handler 가 inherit 결정.

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::core::CoreState;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if let Intent::NewTab { kind, params } = &intent.body {
        new_tab(core, state, engine, kind.as_deref(), params)
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

    // markdown 파일 열기는 최근 목록에 기록(진입점 수렴 — 파일-열기 팝업·CLI 등).
    if kind == "markdown" {
        state.record_recent_markdown(&surface_params);
    }

    let intent = crate::core::intent::DomainIntent::CreateTab {
        pane_id,
        cwd,
        kind: kind.to_string(),
        name: None,
        surface_params,
    };
    if let Err(e) = core.apply(engine, intent) {
        super::report_apply_error(state, &format!("NewTab kind={kind}"), &e);
    }
}
