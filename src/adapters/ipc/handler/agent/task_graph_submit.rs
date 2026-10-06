//! `agent.task_graph_validate` · `agent.task_graph_submit` — v2 task 그래프를 한 번에 검증하고
//! 활성화한다. 검증 오류는 `error.data` 에 실패 단계·task·제출 정의 안의 위치를 싣는다.

use serde_json::{Value, json};
use tasty_agent::task::TaskGraphSpec;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::super::memory::mark_durability;
use super::{agent_err_to_response, now_ms, workspace_id_param};
use crate::app::services::AppServices;
use crate::runtime::engine_access::EngineMut;

/// 그래프를 검증만 한다. 저장소는 바꾸지 않는다.
pub fn handle_task_graph_validate(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    submit(core, engine, id, params, true)
}

/// 그래프를 검증하고 저장한 뒤 활성화한다. 검증에 실패하면 아무것도 저장하지 않는다.
pub fn handle_task_graph_submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    submit(core, engine, id, params, false)
}

fn submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
    dry_run: bool,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let Some(raw) = params.get("graph") else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'graph'");
    };
    let spec: TaskGraphSpec = match serde_json::from_value(raw.clone()) {
        Ok(s) => s,
        Err(e) => return JsonRpcResponse::invalid_params(id, format!("invalid 'graph': {e}")),
    };
    let outcome =
        match core
            .tasks
            .task_graph_submit(engine.task_scope, workspace_id, spec, dry_run, now_ms())
        {
            Ok(o) => o,
            Err(e) => return agent_err_to_response(id, e),
        };
    let tasks = match serde_json::to_value(&outcome.tasks) {
        Ok(v) => v,
        Err(e) => return JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
    };
    let body = if dry_run {
        json!({"valid": true, "activated": false, "tasks": tasks})
    } else {
        json!({"valid": true, "activated": true, "graph_id": outcome.graph_id, "tasks": tasks})
    };
    mark_durability(core, JsonRpcResponse::success(id, body))
}
