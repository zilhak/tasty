//! `workspace_category.*` IPC 핸들러 — 워크스페이스 카테고리(사이드바 폴더) CRUD.
//!
//! **불가침 원칙 1·3 경계**: 카테고리 *CRUD·조회·워크스페이스 소속 변경* 은 에이전트
//! 작업이라 release 허용. 반면 *선택된 카테고리 변경(active)* · *접힘 토글* 은 사용자 UI
//! 상태라 IPC 에 노출하지 않는다. 따라서 본 핸들러의 어떤 연산도 사용자 active/포커스를
//! 바꾸지 않는다 — delete/move 시 워크스페이스 전역 인덱스가 불변이므로 active 도 불변.
//!
//! 카테고리 데이터는 per-engine(`CoreState.categories`) 이지만 **id 는 창을 건너
//! 유일하다** — `IdGenerator.category` 가 공유 카운터다. 그래서 `list` 는 여기서
//! 단일 engine 만 읽고, 전 창 합산은 `app::dispatch::list_global` 이 이 함수를 창마다
//! 불러 합친다. 모든 engine 에 상수로 있는 예약 `normal`(id 0) 만 거기서 한 줄로
//! 접는다. `rename`/`delete` 는 `"id"` 로 소유 창이 지목되므로 포커스에 안 걸린다.
//! 남은 창 의존은 `create`(새 카테고리가 포커스된 창의 engine 에 생기고, 그 창의
//! 워크스페이스만 소속될 수 있다)와 `move`(index 가 창 안의 위치다) 둘이다.

use super::params::{self, p_try};
use serde_json::json;

use crate::core::state::CategoryOpError;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 카테고리 목록 조회(read). 각 카테고리의 워크스페이스 수를 동봉한다.
pub fn handle_list(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let cats: Vec<_> = engine
        .categories()
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let ws_count = engine.workspaces_in_category(c.id).len();
            json!({
                "id": c.id,
                "name": c.name,
                "index": index,
                "collapsed": c.collapsed,
                "is_normal": c.is_normal(),
                "workspace_count": ws_count,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(cats))
}

/// 새 카테고리 생성. `name` 검증(대소문자 무시 중복·예약어 거부) 후 Vec 끝에 추가.
pub fn handle_create(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    match engine.create_category(name) {
        Ok(cat_id) => {
            engine.mark_layout_dirty();
            let name = engine.category_name(cat_id).unwrap_or("").to_string();
            JsonRpcResponse::success(id, json!({ "id": cat_id, "name": name }))
        }
        Err(e) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

/// 카테고리 이름 변경. normal 은 거부.
pub fn handle_rename(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(cat_id) = p_try!(params::opt_int::<u64>(params, "id", &id)) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' parameter");
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    match engine.rename_category(cat_id as u32, name) {
        Ok(()) => {
            engine.mark_layout_dirty();
            JsonRpcResponse::success(id, json!({ "id": cat_id, "name": name }))
        }
        Err(e) => category_err_response(id, e),
    }
}

/// 카테고리 삭제. normal 은 거부. 내부 워크스페이스는 normal 로 귀속(active 불변).
pub fn handle_delete(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let cat_id = match super::params::require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match engine.delete_category(cat_id) {
        Ok(()) => {
            engine.mark_layout_dirty();
            JsonRpcResponse::success(id, json!({ "deleted": true, "id": cat_id }))
        }
        Err(e) => category_err_response(id, e),
    }
}

/// 카테고리 순서 이동(reorder). normal(0번) 위치 고정 — from/to == 0 거부.
pub fn handle_move(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let from = match p_try!(params::opt_int::<u64>(params, "from_index", &id)) {
        Some(f) => f as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'from_index' parameter"),
    };
    let to = match p_try!(params::opt_int::<u64>(params, "to_index", &id)) {
        Some(t) => t as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_index' parameter"),
    };
    match engine.reorder_category(from, to) {
        Ok(()) => {
            engine.mark_layout_dirty();
            JsonRpcResponse::success(id, json!({ "moved": true }))
        }
        Err(e) => category_err_response(id, e),
    }
}

fn category_err_response(id: serde_json::Value, e: CategoryOpError) -> JsonRpcResponse {
    JsonRpcResponse::invalid_params(id, e.to_string())
}
