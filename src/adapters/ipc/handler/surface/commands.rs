use serde_json::json;

use crate::adapters::ipc::handler::params::{self, p_try};
use crate::core::Core;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

/// `surface.commands` — OSC 133 으로 인덱싱된 명령 record 들 (오름차순 시간).
/// `limit` (기본 50), `since` (unix ms updated_at 하한) 지원.
pub(crate) fn handle_commands(
    core: &Core,
    _state: &AppState,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let limit = p_try!(params::opt_int::<usize>(params, "limit", &id));
    let since = p_try!(params::opt_i64(params, "since", &id));
    let entries = match read_command_entries(core, surface_id, limit, since) {
        Ok(v) => v,
        Err(e) => return e.into_response(id),
    };
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "commands": entries }))
}

/// `surface.last_command` — 가장 최근 record. 없으면 `null`.
pub(crate) fn handle_last_command(
    core: &Core,
    _state: &AppState,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let entries = match read_command_entries(core, surface_id, None, None) {
        Ok(v) => v,
        Err(e) => return e.into_response(id),
    };
    let last = entries.into_iter().next_back();
    JsonRpcResponse::success(id, json!({ "surface_id": surface_id, "command": last }))
}

/// `surface.command_at` — 0-based 인덱스 (음수면 끝에서부터). 범위 밖이면 `null`.
pub(crate) fn handle_command_at(
    core: &Core,
    _state: &AppState,
    _engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let index = match p_try!(params::opt_i64(params, "index", &id)) {
        Some(i) => i,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'index' parameter"),
    };
    let entries = match read_command_entries(core, surface_id, None, None) {
        Ok(v) => v,
        Err(e) => return e.into_response(id),
    };
    let len = entries.len() as i64;
    let resolved = if index < 0 { len + index } else { index };
    let cmd = if resolved >= 0 && resolved < len {
        Some(entries[resolved as usize].clone())
    } else {
        None
    };
    JsonRpcResponse::success(
        id,
        json!({ "surface_id": surface_id, "index": index, "command": cmd }),
    )
}

/// Internal helper. memory.list(scope=Surface(N), prefix="tasty.commands.") +
/// 시간 오름차순 + JSON value 만 추출.
struct CommandsReadError {
    message: String,
}
impl CommandsReadError {
    fn into_response(self, id: serde_json::Value) -> JsonRpcResponse {
        JsonRpcResponse::internal_error(id, self.message)
    }
}

fn read_command_entries(
    core: &Core,
    surface_id: u32,
    limit: Option<usize>,
    since: Option<i64>,
) -> Result<Vec<serde_json::Value>, CommandsReadError> {
    let opts = tasty_memory::ListOpts {
        prefix: Some("tasty.commands.".to_string()),
        limit,
        since,
        until: None,
        offset: None,
    };
    let entries =
        match core.with_memory(|s| s.list(&tasty_memory::Scope::Surface(surface_id), &opts)) {
            Ok(v) => v,
            Err(e) => {
                return Err(CommandsReadError {
                    message: format!("memory.list failed: {e}"),
                });
            }
        };
    let out = entries
        .into_iter()
        .filter_map(|e| match e.value {
            tasty_memory::MemoryValue::Json(v) => Some(v),
            tasty_memory::MemoryValue::Text(s) => serde_json::from_str(&s).ok(),
            tasty_memory::MemoryValue::Binary(_) => None,
        })
        .collect();
    Ok(out)
}
