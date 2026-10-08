//! agent.*의 인자를 읽고 협업 기능의 결과를 IPC 응답으로 변환한다.
//! 저장소 조립과 workspace별 영속 처리는 src/core/agent/의 AppServices 메서드가 맡는다.
//! task 실행은 별도의 작업 러너가 담당한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tasty_agent::{AgentError, TaskId};

use tasty_ipc::protocol::JsonRpcResponse;

/// 회차에 묶인 결과 쓰기(완료 보고·결과 제출)를 거절했다. approval.* 의 `-32014`(store_poisoned)와
/// 뜻이 달라 따로 둔다. 도메인 코드 표는 docs/dev-guide/api-conventions.md 에 있다.
const AGENT_ATTEMPT_REJECTED: i32 = -32018;

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn workspace_id_param(params: &Value, id: &Value) -> Result<u32, JsonRpcResponse> {
    super::params::require_u32(params, "workspace_id", id)
}

pub(super) fn task_id_param(params: &Value, id: &Value) -> Result<TaskId, JsonRpcResponse> {
    params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'id' (task id)")
        })
}

pub(super) fn agent_err_to_response(id: Value, err: AgentError) -> JsonRpcResponse {
    use AgentError::*;
    // match에서 err의 일부 필드를 이동하기 전에 메시지를 만든다.
    let msg = err.to_string();
    match err {
        TaskNotFound(_) => JsonRpcResponse::error(id, -32004, msg),
        DependencyCycle(_)
        | UnknownDependency(_)
        | InvalidArgument(_)
        | InvalidTransition { .. } => JsonRpcResponse::invalid_params(id, msg),
        AlreadyTerminal(_) => JsonRpcResponse::error(id, -32008, msg),
        LeaseConflict { .. } => JsonRpcResponse::error(id, -32009, msg),
        LeasePoolExhausted { .. } => JsonRpcResponse::error(id, -32012, msg),
        TaskReferenced { referenced_by, .. } => JsonRpcResponse::error_with_data(
            id,
            -32010,
            msg,
            serde_json::json!({ "referenced_by": referenced_by }),
        ),
        TaskRunning(_) => JsonRpcResponse::error(id, -32011, msg),
        // 요청의 durability 를 고치면 받는다. 대체 모드의 원인을 함께 싣는다.
        StoreNotDurable { cause } => JsonRpcResponse::error_with_data(
            id,
            -32602,
            msg,
            serde_json::json!({"location": "/durability", "store_durable": false, "cause": cause}),
        ),
        // 보고를 적용하지 않았다. 사유와 두 회차 id 로 호출자가 다시 낼지 정한다.
        CompletionRejected {
            attempt_id,
            current_attempt_id,
            reason,
            ..
        } => JsonRpcResponse::error_with_data(
            id,
            AGENT_ATTEMPT_REJECTED,
            msg,
            serde_json::json!({
                "reason": reason,
                "attempt_id": attempt_id,
                "current_attempt_id": current_attempt_id,
            }),
        ),
        // 결과 제출을 받지 않았다. 완료 보고 거절과 같은 코드이고 data 로 사유와 회차를 싣는다.
        SubmissionRejected {
            attempt_id,
            current_attempt_id,
            reason,
            ..
        } => JsonRpcResponse::error_with_data(
            id,
            AGENT_ATTEMPT_REJECTED,
            msg,
            serde_json::json!({
                "reason": reason,
                "attempt_id": attempt_id,
                "current_attempt_id": current_attempt_id,
            }),
        ),
        // report 블록에 쓰지 않았다. 회차에 묶인 쓰기 거절과 같은 코드다.
        ReportRejected {
            task_id,
            attempt,
            reason,
            state,
        } => JsonRpcResponse::error_with_data(
            id,
            AGENT_ATTEMPT_REJECTED,
            msg,
            serde_json::json!({
                "reason": reason,
                "task_id": task_id,
                "attempt": attempt,
                "state": state,
            }),
        ),
        // 실패 단계와 타입 오류(task·경로·기대·실제)를 error.data 로 돌려준다.
        TypeContract(failure) => JsonRpcResponse::error_with_data(
            id,
            -32602,
            msg,
            serde_json::to_value(&*failure).unwrap_or(Value::Null),
        ),
        // 레코드는 이미 써서 그래프가 활성일 수 있다. 호출자가 남은 task 를 찾을 수 있게 graph_id 를 싣는다.
        GraphPartiallyActivated { graph_id, source } => JsonRpcResponse::error_with_data(
            id,
            -32603,
            msg,
            serde_json::json!({
                "graph_id": graph_id,
                "possibly_active": true,
                "cause": source.to_string(),
            }),
        ),
        Memory(_) | Serde(_) => JsonRpcResponse::error(id, -32603, msg),
    }
}

pub(super) fn escape_dot(s: &str) -> String {
    s.replace('"', "\\\"").replace('\n', " ")
}

pub(super) fn name_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing required 'name'"))
}

mod agent_turn;
mod barrier;
mod lease;
mod ratelimit;
mod report;
mod semaphore;
pub(crate) mod task;
mod task_graph_submit;

pub use agent_turn::*;
pub use barrier::*;
pub use lease::*;
pub use ratelimit::*;
pub use report::*;
pub use semaphore::*;
pub use task::*;
pub use task_graph_submit::*;

#[path = "agent/postprocess_ipc_tests.rs"]
#[cfg(test)]
mod postprocess_ipc_tests;

#[path = "agent/report_ipc_tests.rs"]
#[cfg(test)]
mod report_ipc_tests;

#[path = "agent/holding_ttl_ipc_tests.rs"]
#[cfg(test)]
mod holding_ttl_ipc_tests;

#[path = "agent/record_limit_ipc_tests.rs"]
#[cfg(test)]
mod record_limit_ipc_tests;

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tasty_agent::CompletionRejection;

    use super::*;

    #[test]
    fn a_rejected_completion_answers_with_its_reason_and_both_attempts() {
        let err = AgentError::CompletionRejected {
            task_id: "p".into(),
            attempt_id: Some("p#1".into()),
            current_attempt_id: Some("p#2".into()),
            reason: CompletionRejection::StaleAttempt,
        };
        let e = agent_err_to_response(json!(1), err).error.expect("error");
        assert_eq!(e.code, -32018);
        let data = e.data.expect("data");
        assert_eq!(data["reason"], json!("stale_attempt"));
        assert_eq!(data["attempt_id"], json!("p#1"));
        assert_eq!(data["current_attempt_id"], json!("p#2"));
    }
}
