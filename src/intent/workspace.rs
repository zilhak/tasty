//! Workspace 도메인 Intent 핸들러.
//!
//! 정책:
//! - **NewWorkspace**: `kind` None or "terminal" + params empty → `state.add_workspace()`
//!   (사용자 단축키 경로: active 전환 포함). 그 외 kind/params → `add_workspace_background`
//!   (background 경로). IPC `workspace.create` 는 sync return contract 때문에 직접 호출.
//! - **CloseWorkspace**: 미마이그레이션. 사용자 단축키의 cascade 가 `request_close` 결과
//!   에 의존하므로 직접 호출 유지 (intent-exempt).
//! - **ActivateWorkspace**: W1=B per `docs/design/action-dispatch.md` — focus 독립성
//!   원칙. 사용자 단축키/클릭으로만 가능.

use super::{DispatchedIntent, Intent};
use crate::state::AppState;

pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    if let Intent::NewWorkspace { kind, params } = &intent.body {
        new_workspace(state, kind.as_deref(), params);
    }
}

fn new_workspace(state: &mut AppState, kind: Option<&str>, params: &serde_json::Value) {
    let kind = kind.unwrap_or("terminal");
    if kind == "terminal" && params.is_null() {
        // 사용자 경로 — active 전환 포함.
        if let Err(e) = state.add_workspace() {
            tracing::warn!("NewWorkspace terminal failed: {e}");
        }
    } else if let Err(e) = state.add_workspace_background(None, kind, params) {
        tracing::warn!("NewWorkspace kind={kind} failed: {e}");
    }
}
