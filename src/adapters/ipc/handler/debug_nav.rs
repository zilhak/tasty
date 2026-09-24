//! GUI feature 없이도 쓰는 디버그 전용 workspace·tab 전환과 닫기.
//! 헤드리스 시험에서도 서버 로컬 사용자의 동작을 재현한다. release에는 포함하지 않는다.

#![cfg(debug_assertions)]

use super::params::{self, p_try};
use serde_json::json;
use tasty_model::TabSwitch;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 사용자의 탭 전환을 재현한다.
pub(super) fn handle_debug_switch_tab(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match p_try!(params::opt_int::<u64>(params, "index", &id)) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    // 이미 활성인 탭도 성공이다. 범위 오류와 구별해 switched=false를 반환한다.
    match state.goto_tab_in_pane(engine, index) {
        TabSwitch::Switched => {
            JsonRpcResponse::success(id, json!({"switched": true, "active": index}))
        }
        TabSwitch::AlreadyActive => {
            JsonRpcResponse::success(id, json!({"switched": false, "active": index}))
        }
        TabSwitch::OutOfRange { tabs } => JsonRpcResponse::invalid_params(
            id,
            format!("Tab index {index} out of range — the focused pane has {tabs} tab(s)"),
        ),
        TabSwitch::NoPane => JsonRpcResponse::invalid_params(id, "No focused pane"),
    }
}

/// 사용자 메뉴의 workspace 닫기를 재현한다. 복원 기록과 포커스가 바뀌므로 디버그 전용이다.
pub(super) fn handle_debug_close_workspace(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match p_try!(params::opt_int::<u64>(params, "index", &id)) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    if index >= engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {index} out of range"),
        );
    }
    // 실제 메뉴와 달리 창 종료는 재현하지 않으므로 마지막 workspace는 남긴다.
    if engine.workspaces.len() == 1 {
        return JsonRpcResponse::invalid_params(
            id,
            "Refusing to close the last workspace (would leave no workspace)",
        );
    }
    let closed = state.close_workspace_at(engine, index, crate::state::WorkspaceCloseOrigin::User);
    JsonRpcResponse::success(id, json!({"closed": closed, "index": index}))
}

/// 사용자의 workspace 전환을 재현한다. OS 창 포커스는 바꾸지 않는다.
pub(super) fn handle_debug_switch_workspace(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let index = match p_try!(params::opt_int::<u64>(params, "index", &id)) {
        Some(i) => i as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    if index >= engine.workspaces.len() {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Workspace index {index} out of range"),
        );
    }
    state.switch_workspace(engine, index);
    JsonRpcResponse::success(id, json!({"switched": true, "active": index}))
}
