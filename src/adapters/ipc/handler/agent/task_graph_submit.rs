//! `agent.task_graph_validate` · `agent.task_graph_submit` — v2 task 그래프를 한 번에 검증하고
//! 활성화한다. 검증 오류는 `error.data` 에 실패 단계·task·제출 정의 안의 위치를 싣는다.

use serde_json::{Value, json};
use tasty_agent::task::{GraphDurability, TaskGraphSpec};
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::super::memory::mark_durability;
use super::{agent_err_to_response, now_ms, reply_task, workspace_id_param};
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
    submit(core, engine, id, params, true, |_, id, s| {
        JsonRpcResponse::success(
            id,
            json!({"valid": true, "activated": false, "durability": s.durability, "tasks": s.tasks}),
        )
    })
}

/// 그래프를 검증하고 저장한 뒤 활성화한다. 검증에 실패하면 아무것도 저장하지 않는다.
/// 활성화한 task 는 commit 뒤에만 runner 의 실행 대상이 된다.
pub fn handle_task_graph_submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    submit(core, engine, id, params, false, |core, id, s| {
        mark_durability(
            core,
            JsonRpcResponse::success(
                id,
                json!({"valid": true, "activated": true, "graph_id": s.graph_id,
                       "durability": s.durability, "tasks": s.tasks}),
            ),
        )
    })
}

struct Submitted {
    graph_id: String,
    durability: GraphDurability,
    tasks: Value,
}

/// 검증(과 dry_run 이 아니면 저장·활성화)을 writer 순서에서 하고 그래프 id 와 task 목록으로 응답한다.
/// 저장소가 영속이 아닐 때의 판정은 TaskService 가 한다.
fn submit(
    core: &AppServices,
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
    dry_run: bool,
    respond: impl FnOnce(&AppServices, Value, Submitted) -> JsonRpcResponse + Send + 'static,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let Some(raw) = params.get("graph") else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'graph'");
    };
    let spec = match TaskGraphSpec::from_json(raw) {
        Ok(spec) => spec,
        Err(e) => return agent_err_to_response(id, e),
    };
    let durability = spec.durability;
    let scope = engine.task_scope.clone();
    reply_task(
        core,
        id,
        params,
        move |tasks| tasks.task_graph_submit(&scope, workspace_id, spec, dry_run, now_ms()),
        move |core, id, outcome| match serde_json::to_value(&outcome.tasks) {
            Ok(tasks) => respond(
                core,
                id,
                Submitted {
                    graph_id: outcome.graph_id,
                    durability,
                    tasks,
                },
            ),
            Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
        },
    )
}

#[cfg(test)]
mod tests {
    use tasty_agent::AgentError;

    use super::*;

    // 형식 오류도 의미 검증 오류처럼 error.data 에 위치와 task id 를 싣는다.
    #[test]
    fn a_malformed_graph_answers_with_the_location() {
        let graph = json!({"contract_version": 2, "tasks": [{"id": "x",
            "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}, "bogus": 1}]});
        let err = TaskGraphSpec::from_json(&graph).expect_err("unknown key");
        let e = agent_err_to_response(json!(7), err).error.expect("error");
        assert_eq!(e.code, -32602);
        let data = e.data.expect("data");
        assert_eq!(data["location"], json!("/tasks/0/bogus"));
        assert_eq!(data["task_id"], json!("x"));
        assert_eq!(data["stage"], json!("input"));
    }

    // command 안의 모르는 키도 그래프 제출에서는 위치를 실어 거절한다.
    #[test]
    fn an_unknown_command_key_answers_with_the_location() {
        let graph = json!({"contract_version": 2, "tasks": [{"id": "x",
            "command": {"kind": "run", "workspace_id": 1, "command": ["true"], "bogus": 1}}]});
        let err = TaskGraphSpec::from_json(&graph).expect_err("unknown command key");
        let e = agent_err_to_response(json!(7), err).error.expect("error");
        assert_eq!(e.code, -32602);
        let data = e.data.expect("data");
        assert_eq!(data["location"], json!("/tasks/0/command/bogus"));
        assert_eq!(data["task_id"], json!("x"));
        assert_eq!(data["stage"], json!("input"));
    }

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
