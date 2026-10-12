//! `memory.cache.*` IPC handlers.

use crate::adapters::ipc::handler::params::{self, p_try};
use serde_json::{Value, json};
use tasty_memory::cache as cache_mod;

use crate::app::services::AppServices;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{entry_to_json, map_error_ref, parse_value, require_str, require_workspace_id};

pub fn handle_cache_put(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let value = match parse_value(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ttl_secs = match p_try!(params::opt_int::<u64>(params, "ttl_secs", &id)) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'ttl_secs' (>0)");
        }
    };
    let owner = caller.owner().to_string();
    core.reply_unit(
        id,
        super::super::deferred::params_bytes(params),
        move |s| cache_mod::cache_put(s, &owner, workspace_id, &key, &value, ttl_secs),
        |core, id, result| match result {
            Ok(version) => super::written(core, id, json!({ "ok": true, "version": version })),
            Err(e) => map_error_ref(id, e),
        },
    )
}

pub fn handle_cache_get(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    core.reply_unit(
        id,
        super::super::deferred::params_bytes(params),
        move |s| cache_mod::cache_get(s, workspace_id, &key),
        |_, id, result| match result {
            Ok(Some(entry)) => JsonRpcResponse::success(id, entry_to_json(&entry)),
            Ok(None) => JsonRpcResponse::success(id, Value::Null),
            Err(e) => map_error_ref(id, e),
        },
    )
}

pub fn handle_cache_invalidate(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let key = match require_str(params, "key", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    core.reply_unit(
        id,
        super::super::deferred::params_bytes(params),
        move |s| cache_mod::cache_invalidate(s, &owner, workspace_id, &key),
        |core, id, result| match result {
            Ok(()) => super::written(core, id, json!({ "ok": true })),
            Err(e) => map_error_ref(id, e),
        },
    )
}

pub fn handle_cache_clear(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let owner = caller.owner().to_string();
    core.reply_unit(
        id,
        super::super::deferred::params_bytes(params),
        move |s| cache_mod::cache_clear(s, &owner, workspace_id),
        |core, id, result| match result {
            Ok(removed) => super::written(core, id, json!({ "ok": true, "removed": removed })),
            Err(e) => map_error_ref(id, e),
        },
    )
}

pub fn handle_cache_list(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match require_workspace_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    core.reply_unit(
        id,
        super::super::deferred::params_bytes(params),
        move |s| cache_mod::cache_list(s, workspace_id),
        |_, id, result| match result {
            Ok(keys) => JsonRpcResponse::success(id, json!({ "keys": keys, "count": keys.len() })),
            Err(e) => map_error_ref(id, e),
        },
    )
}
