//! Debug 빌드 전용 IPC 핸들러 중 **gui feature 에 의존하지 않는 것** — 워크스페이스
//! 전환/닫기 · 탭 전환.
//!
//! 형제 모듈 `debug.rs` 와 갈라져 있는 이유는 게이트 하나뿐이다. `debug.rs` 의
//! 핸들러 상당수는 `state.popups` / `state.banners` / `state.modifier_hint` 처럼
//! gui feature 에서만 존재하는 필드를 만져서 모듈 전체가
//! `#[cfg(all(debug_assertions, feature = "gui"))]` 로 걸려 있다. 여기 셋은
//! `AppState::switch_workspace` / `goto_tab_in_pane` / `close_workspace_at` 와
//! `engine.workspaces` 만 쓰고 그 어느 것에도 feature 게이트가 없다 — 즉 gui 를
//! 끈 빌드에서도 그대로 컴파일되고 동작한다.
//!
//! 갈라 둔 실익: headless **debug** 데몬에서도 이 셋이 라우터에 등록된다.
//! `tests/attach_attention_loopback.rs` 의
//! `hard_occupied_attention_survives_the_servers_local_focus` 는 서버 쪽 로컬
//! 포커스 이동을 `debug.switch_workspace` 로 재현하는데, 모듈이 gui 게이트에
//! 묶여 있던 동안 헤드리스 조합에서는 `Method not found` 로 죽었다.
//!
//! 불가침 원칙 1(사용자 입력 재현은 release 에 없다)은 그대로 지켜진다 — 이
//! 파일은 `#![cfg(debug_assertions)]` 이라 release 빌드에는 아예 없다. 원칙이
//! 가르는 축은 debug/release 이지 gui/headless 가 아니다
//! (`docs/dev-guide/debug-ipc.md`).

#![cfg(debug_assertions)]

use super::params::{self, p_try};
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 포커스 pane 의 활성 탭 전환 — 사용자의 탭 클릭 재현. release 미노출
/// ([`handle_debug_switch_workspace`] 의 탭 대응). egui-mesh 텍스처 상태의 탭
/// 전환/복귀 검증 등 탭 가시성 시나리오 재현에 쓴다.
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
    if state.goto_tab_in_pane(engine, index) {
        JsonRpcResponse::success(id, json!({"switched": true, "active": index}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Tab index {index} out of range"))
    }
}

/// 워크스페이스 close — 사용자의 워크스페이스 컨텍스트 메뉴 "Close workspace"
/// (`src/view/main/redraw.rs` 의 native 메뉴 응답 `Some(6)`) 재현. release 미노출.
///
/// release IPC 의 `surface.close` 로는 이 경로에 도달할 수 없다 — cascade close 는
/// **탭/페인이 하나만 남았을 때만** workspace 단계까지 올라가므로 cleanup 대상이
/// 항상 surface 1개다. "탭이 많은 워크스페이스를 통째로 닫는" 비용(close 계측
/// `path="gui"`)은 이 메뉴 항목으로만 발생하고, 그래서 계측 기준선을 잡으려면
/// 이 항목을 재현할 수단이 필요하다.
///
/// 사용자 상태(closed_items undo 스택 / 포커스)를 건드리는 사용자 행동이므로
/// release 표면에는 두지 않는다 (CLAUDE.md "사용자 행동 ↔ 에이전트 행동 분리").
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
    // GUI 메뉴 경로는 마지막 workspace 를 닫으면 `request_close()` 로 창까지 닫는다.
    // debug IPC 는 그 창 종료까지 재현하지 않으므로, workspaces 가 비어 다음 redraw
    // 의 `active_workspace()` 가 패닉하는 상태를 만들지 않도록 거절한다.
    if engine.workspaces.len() == 1 {
        return JsonRpcResponse::invalid_params(
            id,
            "Refusing to close the last workspace (would leave no workspace)",
        );
    }
    let closed = state.close_workspace_at(engine, index, crate::state::WorkspaceCloseOrigin::User);
    JsonRpcResponse::success(id, json!({"closed": closed, "index": index}))
}

/// 워크스페이스 활성 전환 — 사용자의 포커스 조작(워크스페이스 전환) 재현. release 미노출.
/// `active_workspace` 인덱스 변경뿐이라 OS 의존성 없음.
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
