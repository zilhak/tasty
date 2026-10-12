use serde_json::{Value, json};

use crate::adapters::ipc::handler::params::{self, p_try};
use crate::app::services::AppServices;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::super::memory::mark_durability;
use super::{name_param, now_ms, reply_agent, workspace_id_param};

fn serialize<T: serde::Serialize>(id: Value, value: T) -> JsonRpcResponse {
    match serde_json::to_value(value) {
        Ok(v) => JsonRpcResponse::success(id, v),
        Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
    }
}

pub fn handle_semaphore_create(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let permits = match p_try!(params::opt_int::<u64>(params, "permits", &id)) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'permits' (must be u32 >= 1)",
            );
        }
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| {
            crate::app::services::semaphore::create(
                mem,
                workspace_id,
                name.clone(),
                permits,
                now_ms(),
            )
        },
        move |core, id, s| mark_durability(core, serialize(id, s)),
    )
}

pub fn handle_semaphore_set_permits(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let permits = match p_try!(params::opt_int::<u64>(params, "permits", &id)) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'permits' (must be u32 >= 1)",
            );
        }
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| {
            crate::app::services::semaphore::set_permits(
                mem,
                workspace_id,
                &name,
                permits,
                now_ms(),
            )
        },
        move |core, id, s| mark_durability(core, serialize(id, s)),
    )
}

pub fn handle_semaphore_acquire(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    let ttl_ms = match params::opt_int::<u64>(params, "ttl_ms", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| {
            crate::app::services::semaphore::acquire(
                mem,
                workspace_id,
                &name,
                &holder,
                ttl_ms,
                now_ms(),
            )
        },
        move |core, id, outcome| mark_durability(core, serialize(id, outcome)),
    )
}

pub fn handle_semaphore_release(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let holder = match params.get("holder").and_then(|v| v.as_str()) {
        Some(h) if !h.is_empty() => h.to_string(),
        _ => return JsonRpcResponse::invalid_params(id, "Missing or empty 'holder'"),
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::semaphore::release(mem, workspace_id, &name, &holder),
        move |core, id, outcome| mark_durability(core, serialize(id, outcome)),
    )
}

pub fn handle_semaphore_list(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::semaphore::list(mem, workspace_id, now_ms()),
        move |_, id, semaphores| {
            JsonRpcResponse::success(
                id,
                json!({ "total": semaphores.len(), "semaphores": semaphores }),
            )
        },
    )
}

pub fn handle_semaphore_delete(
    core: &AppServices,
    _engine: &crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match name_param(params, &id) {
        Ok(n) => n,
        Err(e) => return e,
    };
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::semaphore::delete(mem, workspace_id, &name),
        move |core, id, ()| {
            mark_durability(
                core,
                JsonRpcResponse::success(id, json!({ "deleted": true })),
            )
        },
    )
}
