//! Tab 도메인 Intent 핸들러.
//!
//! 정책:
//! - **NewTab**: `kind` None → "terminal" fallback. terminal kind 는 host PTY spawn
//!   경로 (`state.add_tab_to_pane`), 그 외는 `state.add_kind_tab` (focused pane).
//!   focused 의존이므로 사용자 동작 전용 (단축키/메뉴/파일 드롭). agent 가 명시
//!   pane 에 추가하려면 IPC `tab.new` (`add_kind_tab_to_pane`) 를 사용한다.
//! - **CloseTab**: ID 지정. close_tab_by_tab_id 호출. fire-and-forget.

use super::{DispatchedIntent, Intent};
use crate::state::AppState;

pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    match &intent.body {
        Intent::NewTab { kind, params } => new_tab(state, kind.as_deref(), params),
        Intent::CloseTab { tab_id } => close_tab(state, *tab_id),
        _ => {}
    }
}

fn new_tab(state: &mut AppState, kind: Option<&str>, params: &serde_json::Value) {
    let kind = kind.unwrap_or("terminal");
    if kind == "terminal" {
        // terminal 은 host PTY spawn 경로 (cwd None → 기본 inherit).
        let pane_id = state.active_workspace(engine).focused_pane;
        if let Err(e) = state.add_tab_to_pane(engine, pane_id, None) {
            tracing::warn!("NewTab terminal failed: {e}");
        }
    } else if let Err(e) = state.add_kind_tab(engine, kind, params) {
        tracing::warn!("NewTab kind={kind} failed: {e}");
    }
}

fn close_tab(state: &mut AppState, tab_id: u32) {
    state.close_tab_by_tab_id(engine, tab_id);
}
