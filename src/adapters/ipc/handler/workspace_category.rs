//! 카테고리 데이터와 workspace 소속을 관리한다. 사용자 선택·접힘 상태는 IPC로 바꾸지 않는다.
//! 데이터는 engine별로 저장하지만 ID는 창 간에 유일하다. 목록은 상위 라우터가 합산하며
//! 공통 normal(id0)은 하나로 합친다. rename/delete는 ID로 창을 선택한다.
//! create는 포커스된 창에 만들고, move의 from_index 호환 입력도 해당 창을 사용한다.

use super::params::{self, p_try};
use crate::runtime::engine_access::EngineMut;
use serde_json::json;

use crate::app::command::DomainIntent;
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

