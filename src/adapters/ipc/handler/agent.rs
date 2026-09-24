//! agent.*의 인자를 읽고 협업 기능의 결과를 IPC 응답으로 변환한다.
//! 저장소 조립과 workspace별 영속 처리는 src/core/agent/의 Core 메서드가 맡는다.
//! task 실행은 별도의 작업 러너가 담당한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tasty_agent::{AgentError, TaskId};

use tasty_ipc::protocol::JsonRpcResponse;

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

mod barrier;
mod lease;
mod ratelimit;
mod semaphore;
pub(crate) mod task;

pub use barrier::*;
pub use lease::*;
pub use ratelimit::*;
pub use semaphore::*;
pub use task::*;
