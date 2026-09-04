//! `agent.*` IPC 핸들러 — 협업 primitive.
//!
//! 본 모듈은 `tasty-agent` 의 도메인 모델을 IPC 표면으로 노출한다. 영속은
//! `tasty-memory` 의 workspace scope 에 위임. **task 실행 자체는 본 phase
//! 5.1 범위 밖** — 호스트가 `Ready` task 를 골라 실제 IPC dispatch 를 트리거하는
//! 스케줄러 루프는 후속 5.x 에서 붙는다. 본 단계는 state 머신 / DAG / 영속
//! 정확성만 보장한다.
//!
//! handler 는 param 파싱과 응답 직렬화만 담당. `with_memory + ...Store::new`
//! 의 store 조립은 `src/core/agent/` 의 Core extension 메서드 (`Core::task_*`,
//! `Core::barrier_*`, `Core::lease_*`, `Core::rate_limit_*`,
//! `Core::semaphore_*`) 가 책임진다.

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
    // 메시지를 먼저 뽑아둔다 — 아래 `TaskReferenced { referenced_by, .. }` 처럼
    // 필드를 값으로 바인딩하는 arm 이 있으면, match 안에서 `err.to_string()` 을
    // 또 부르는 건 partial move 이후라 컴파일이 안 된다.
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
