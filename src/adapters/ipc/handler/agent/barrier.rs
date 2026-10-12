use serde_json::{Value, json};

use crate::adapters::ipc::handler::params::{self, p_try};
use crate::app::services::AppServices;
use crate::runtime::engine_access::EngineMut;
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

pub fn handle_barrier_create(
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
    let count_required = match p_try!(params::opt_int::<u64>(params, "count_required", &id)) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'count_required' (must be u32 >= 1)",
            );
        }
    };
    let timeout_ms = p_try!(params::opt_int::<u64>(params, "timeout_ms", &id));
    reply_agent(
        core,
        id,
        params,
        move |mem| {
            crate::app::services::barrier::create(
                mem,
                workspace_id,
                name.clone(),
                count_required,
                timeout_ms,
                now_ms(),
            )
        },
        move |core, id, b| mark_durability(core, serialize(id, b)),
    )
}

pub fn handle_barrier_signal(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
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
    let events = std::sync::Arc::clone(engine.task_scope.event_queue());
    reply_agent(
        core,
        id,
        params,
        move |mem| crate::app::services::barrier::signal(mem, workspace_id, &name, now_ms()),
        move |core, id, b| {
            // 신호가 요구 수를 채우면 commit 뒤에 Closed 이벤트를 넣는다.
            if b.state == tasty_agent::BarrierState::Closed {
                events.push(tasty_task_runtime::event_feed::AgentEvent::BarrierClosed {
                    workspace_id,
                    name: b.name.clone(),
                    count_required: b.count_required,
                });
            }
            mark_durability(core, serialize(id, b))
        },
    )
}

pub fn handle_barrier_state(
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
        move |mem| crate::app::services::barrier::state(mem, workspace_id, &name, now_ms()),
        move |_, id, b| serialize(id, b),
    )
}

/// 완료까지 기다리지 않고 현재 barrier 상태를 반환한다.
pub fn handle_barrier_await(
    core: &AppServices,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_barrier_state(core, engine, caller, id, params)
}

pub fn handle_barrier_list(
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
        move |mem| crate::app::services::barrier::list(mem, workspace_id, Some(now_ms())),
        move |_, id, barriers| {
            JsonRpcResponse::success(id, json!({ "total": barriers.len(), "barriers": barriers }))
        },
    )
}

pub fn handle_barrier_delete(
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
        move |mem| crate::app::services::barrier::delete(mem, workspace_id, &name),
        move |core, id, ()| {
            mark_durability(
                core,
                JsonRpcResponse::success(id, json!({ "deleted": true })),
            )
        },
    )
}
