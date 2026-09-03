//! `memory.goal_*` IPC handlers — surface 스코프 goal 오버레이.
//!
//! surface id 는 **명시 params 필수**다(`require_surface_id`). 활성 surface 로
//! 폴백하지 않는다 — 포커스 독립성(`docs/design/policies/focus.md`).

use serde_json::{Value, json};
use tasty_memory::goal as goal_mod;

use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{entry_to_json, map_error, require_str, require_surface_id};

pub fn handle_goal_set(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let goal = match require_str(params, "goal", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| goal_mod::goal_set(s, &owner, surface_id, &goal)) {
        Ok(version) => JsonRpcResponse::success(id, json!({ "ok": true, "version": version })),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_goal_get(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match core.with_memory(|s| goal_mod::goal_get(s, surface_id)) {
        Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
        Ok(None) => JsonRpcResponse::success(id, Value::Null),
        Err(e) => map_error(id, e),
    }
}

pub fn handle_goal_clear(
    core: &Core,
    _state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    match core.with_memory(|s| goal_mod::goal_clear(s, &owner, surface_id)) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "ok": true })),
        Err(e) => map_error(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::super::require_surface_id;
    use serde_json::json;

    #[test]
    fn require_surface_id_accepts_valid() {
        let id = json!(1);
        assert_eq!(
            require_surface_id(&json!({ "surface_id": 7 }), &id).unwrap(),
            7
        );
    }

    #[test]
    fn require_surface_id_rejects_missing_or_out_of_range() {
        let id = json!(1);
        // 누락
        assert!(require_surface_id(&json!({}), &id).is_err());
        // 타입 불일치
        assert!(require_surface_id(&json!({ "surface_id": "3" }), &id).is_err());
        // 음수 / u32 범위 초과
        assert!(require_surface_id(&json!({ "surface_id": -1 }), &id).is_err());
        assert!(
            require_surface_id(&json!({ "surface_id": u64::from(u32::MAX) + 1 }), &id).is_err()
        );
    }

    #[test]
    fn require_surface_id_rejects_pty_id_space() {
        use crate::core::pty_registry::PTY_ID_BASE;
        let id = json!(1);
        assert!(require_surface_id(&json!({ "surface_id": PTY_ID_BASE }), &id).is_err());
        assert!(require_surface_id(&json!({ "surface_id": 2147484147u64 }), &id).is_err());
        assert_eq!(
            require_surface_id(&json!({ "surface_id": PTY_ID_BASE - 1 }), &id).unwrap(),
            PTY_ID_BASE - 1
        );
    }

    #[test]
    fn scope_param_rejects_pty_id_space_surface() {
        use super::super::{optional_scope, require_scope};
        use crate::core::pty_registry::PTY_ID_BASE;
        let id = json!(1);
        let polluted = json!({ "scope": format!("surface:{}", PTY_ID_BASE) });
        assert!(require_scope(&polluted, &id).is_err());
        assert!(optional_scope(&polluted, &id).is_err());

        let ok = json!({ "scope": "surface:7" });
        assert!(require_scope(&ok, &id).is_ok());
        assert!(optional_scope(&ok, &id).unwrap().is_some());
        // scope 미지정은 여전히 None.
        assert!(optional_scope(&json!({}), &id).unwrap().is_none());
    }
}
