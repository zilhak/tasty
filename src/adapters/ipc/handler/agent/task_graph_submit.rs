//! `agent.task_graph_validate` · `agent.task_graph_submit` — v2 task 그래프를 한 번에 검증하고
//! 활성화한다. 검증 오류는 `error.data` 에 실패 단계·task·제출 정의 안의 위치를 싣는다.

use serde_json::{Value, json};
use tasty_agent::task::{GraphDurability, TaskGraphSpec};
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
    match submit(core, engine, &id, params, true) {
        Ok(s) => JsonRpcResponse::success(
            id,
            json!({"valid": true, "activated": false, "durability": s.durability, "tasks": s.tasks}),
        ),
        Err(e) => e,
    }
}

/// 그래프를 검증하고 저장한 뒤 활성화한다. 검증에 실패하면 아무것도 저장하지 않는다.
pub fn handle_task_graph_submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    match submit(core, engine, &id, params, false) {
        Ok(s) => mark_durability(
            core,
            JsonRpcResponse::success(
                id,
                json!({"valid": true, "activated": true, "graph_id": s.graph_id,
                       "durability": s.durability, "tasks": s.tasks}),
            ),
        ),
        Err(e) => e,
    }
}

struct Submitted {
    graph_id: String,
    durability: GraphDurability,
    tasks: Value,
}

/// 검증(과 dry_run 이 아니면 저장·활성화)을 하고 그래프 id 와 task 목록을 돌려준다.
/// 저장소가 영속이 아닐 때의 판정은 TaskService 가 한다.
fn submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    id: &Value,
    params: &Value,
    dry_run: bool,
) -> Result<Submitted, JsonRpcResponse> {
    let workspace_id = workspace_id_param(params, id)?;
    let Some(raw) = params.get("graph") else {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            "Missing required 'graph'",
        ));
    };
    let spec: TaskGraphSpec = serde_json::from_value(raw.clone()).map_err(|e| {
        JsonRpcResponse::invalid_params(id.clone(), format!("invalid 'graph': {e}"))
    })?;
    let durability = spec.durability;
    let outcome = core
        .tasks
        .task_graph_submit(engine.task_scope, workspace_id, spec, dry_run, now_ms())
        .map_err(|e| agent_err_to_response(id.clone(), e))?;
    let tasks = serde_json::to_value(&outcome.tasks)
        .map_err(|e| JsonRpcResponse::error(id.clone(), -32603, format!("serialize: {e}")))?;
    Ok(Submitted {
        graph_id: outcome.graph_id,
        durability,
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use tasty_agent::AgentError;

    use super::*;

    #[test]
    fn a_partially_activated_graph_answers_with_its_graph_id() {
        let err = AgentError::GraphPartiallyActivated {
            graph_id: "g-1-000003".into(),
            source: Box::new(AgentError::InvalidArgument("disk".into())),
        };
        let resp = agent_err_to_response(json!(7), err);
        let e = resp.error.expect("error");
        assert_eq!(e.code, -32603);
        assert!(e.message.contains("g-1-000003"), "{}", e.message);
        let data = e.data.expect("data");
        assert_eq!(data["graph_id"], json!("g-1-000003"));
        assert_eq!(data["possibly_active"], json!(true));
        assert!(data["cause"].as_str().unwrap().contains("disk"), "{data}");
    }
}
