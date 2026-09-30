//! 카테고리 데이터와 workspace 소속을 관리한다. 사용자 선택·접힘 상태는 IPC로 바꾸지 않는다.
//! 데이터는 engine별로 저장하지만 ID는 창 간에 유일하다. 목록은 상위 라우터가 합산하며
//! 공통 normal(id0)은 하나로 합친다. rename/delete는 ID로 창을 선택한다.
//! create는 포커스된 창에 만들고, move의 from_index 호환 입력도 해당 창을 사용한다.

use super::params::{self, p_try};
use crate::core::engine_access::EngineMut;
use serde_json::json;

use crate::core::intent::DomainIntent;
use tasty_ipc::protocol::JsonRpcResponse;

/// 카테고리 목록 조회(read). 각 카테고리의 워크스페이스 수를 동봉한다.
pub fn handle_list(
    presentation: &dyn crate::model::StructurePresentation,
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
                "collapsed": presentation.category_collapsed(c.id),
                "is_normal": c.is_normal(),
                "workspace_count": ws_count,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(cats))
}

/// 새 카테고리 생성. `name` 검증(대소문자 무시 중복·예약어 거부) 후 Vec 끝에 추가.
pub fn handle_create(
    core: &mut crate::core::Core,
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let intent = DomainIntent::CreateCategory {
        name: name.to_string(),
    };
    if let Err(e) = core.apply(engine, intent) {
        return JsonRpcResponse::invalid_params(id, e.to_string());
    }
    // Core는 새 카테고리를 목록 끝에 추가한다.
    let Some(cat) = engine.categories().last() else {
        return JsonRpcResponse::internal_error(id, "category list is empty after create");
    };
    JsonRpcResponse::success(id, json!({ "id": cat.id, "name": cat.name }))
}

/// 카테고리 이름 변경. normal 은 거부.
pub fn handle_rename(
    core: &mut crate::core::Core,
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(cat_id) = p_try!(params::opt_int::<u64>(params, "id", &id)) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' parameter");
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let intent = DomainIntent::RenameCategory {
        id: cat_id as u32,
        name: name.to_string(),
    };
    match core.apply(engine, intent) {
        Ok(_) => JsonRpcResponse::success(id, json!({ "id": cat_id, "name": name })),
        Err(e) => category_err_response(id, e),
    }
}

/// 카테고리 삭제. normal 은 거부. 내부 워크스페이스는 normal 로 귀속(active 불변).
pub fn handle_delete(
    core: &mut crate::core::Core,
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let cat_id = match super::params::require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match core.apply(engine, DomainIntent::DeleteCategory { id: cat_id }) {
        Ok(_) => JsonRpcResponse::success(id, json!({ "deleted": true, "id": cat_id })),
        Err(e) => category_err_response(id, e),
    }
}

/// normal의 위치는 고정한다. ID를 지정하면 소유 창을 선택하고 from_index는 포커스된 창을 쓴다.
/// 둘을 함께 지정하면 거절한다. to_index는 선택한 창 안의 목적지다.
pub fn handle_move(
    core: &mut crate::core::Core,
    engine: &mut EngineMut<'_>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let named = p_try!(params::opt_int::<u64>(params, "id", &id));
    let from_index = p_try!(params::opt_int::<u64>(params, "from_index", &id));
    let from = match (named, from_index) {
        (Some(_), Some(_)) => {
            return JsonRpcResponse::invalid_params(
                id,
                "give either 'id' (the category to move) or 'from_index', not both",
            );
        }
        (Some(cat_id), None) => match engine.category_index(cat_id as u32) {
            Some(i) => i,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("no workspace category {cat_id}"),
                );
            }
        },
        (None, Some(f)) => f as usize,
        (None, None) => {
            return JsonRpcResponse::invalid_params(id, "Missing 'id' or 'from_index' parameter");
        }
    };
    let to = match p_try!(params::opt_int::<u64>(params, "to_index", &id)) {
        Some(t) => t as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_index' parameter"),
    };
    let intent = DomainIntent::ReorderCategory {
        from_index: from,
        to_index: to,
    };
    match core.apply(engine, intent) {
        Ok(_) => JsonRpcResponse::success(id, json!({ "moved": true })),
        Err(e) => category_err_response(id, e),
    }
}

/// Core가 CategoryOpError의 문구를 그대로 담아 돌려준다.
fn category_err_response(id: serde_json::Value, e: anyhow::Error) -> JsonRpcResponse {
    JsonRpcResponse::invalid_params(id, e.to_string())
}
