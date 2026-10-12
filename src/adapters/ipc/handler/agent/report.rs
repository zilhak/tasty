//! DAG report 의 IPC. custom 기록 append(`agent.report_append`)와 조회(`agent.dag_report`).
//! 저장 규칙은 `tasty_agent::task::report` 에 있다.

use serde_json::{Value, json};
use tasty_agent::task::report::ReportAddress;
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use crate::app::services::AppServices;
use crate::runtime::engine_access::EngineMut;

use super::super::memory::mark_durability;
use super::reply_task;
use crate::adapters::ipc::handler::params::optional_u32;

/// `address`(자식이 받은 `TASTY_TASK_REPORT` 값)가 가리키는 블록에 `text` 를 더한다. 상한에 걸려
/// 잘리거나 저장하지 않아도 성공이며, 결과의 `result` 가 `stored`·`omitted` 를 알린다.
pub fn handle_report_append(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(address) = params.get("address").and_then(Value::as_str) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'address'");
    };
    let Some(text) = params.get("text").and_then(Value::as_str) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'text'");
    };
    let addr = match ReportAddress::parse(address) {
        Ok(a) => a,
        Err(e) => return JsonRpcResponse::invalid_params(id, e),
    };
    let writer = match super::agent_turn::session_writer(caller) {
        Ok(w) => w,
        Err(agent_id) => {
            return JsonRpcResponse::error(
                id,
                -32001,
                format!("agent '{agent_id}' is not a session that can write a task report"),
            );
        }
    };
    let scope = engine.task_scope.clone();
    let text = text.to_string();
    reply_task(
        core,
        id,
        params,
        move |tasks| tasks.report_append(&scope, &addr, &text, writer),
        |core, id, outcome| mark_durability(core, JsonRpcResponse::success(id, json!(outcome))),
    )
}

/// DAG 하나의 report. `task` 로 task 하나, `attempt` 로 그 task 의 회차 하나를 고른다.
pub fn handle_dag_report(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match optional_u32(params, "workspace_id", &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let attempt = match optional_u32(params, "attempt", &id) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let Some(dag_id) = params.get("id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' (dag id)");
    };
    let task = params.get("task").and_then(Value::as_str);
    if attempt.is_some() && task.is_none() {
        return JsonRpcResponse::invalid_params(id, "'attempt' needs 'task'");
    }
    let include_raw = params
        .get("include_raw")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let workspaces = crate::app::task_completion::dag_workspaces(
        engine.workspaces().into_iter().map(|w| w.id),
        workspace_id,
    );
    let scope = engine.task_scope.clone();
    let dag_id = dag_id.to_string();
    let task = task.map(str::to_string);
    reply_task(
        core,
        id,
        params,
        move |tasks| {
            Ok(
                match tasks.dag_report(
                    &scope,
                    &workspaces,
                    &dag_id,
                    task.as_deref(),
                    attempt,
                    include_raw,
                )? {
                    Some(report) => Ok(report),
                    None => Err(dag_id),
                },
            )
        },
        |_, id, found| match found {
            Ok(report) => JsonRpcResponse::success(id, report),
            Err(dag_id) => JsonRpcResponse::invalid_params(id, format!("unknown dag id: {dag_id}")),
        },
    )
}
