//! `agent.*` IPC 핸들러 — 협업 primitive (Phase 5).
//!
//! 본 모듈은 `tasty-agent` 의 도메인 모델을 IPC 표면으로 노출한다. 영속은
//! `tasty-memory` 의 workspace scope 에 위임. **task 실행 자체는 본 phase
//! 5.1 범위 밖** — 호스트가 `Ready` task 를 골라 실제 IPC dispatch 를 트리거하는
//! 스케줄러 루프는 후속 5.x 에서 붙는다. 본 단계는 state 머신 / DAG / 영속
//! 정확성만 보장한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tasty_agent::{AgentError, TaskId, TaskStore};
use tasty_memory::with_store;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn workspace_id_param(params: &Value, id: &Value) -> Result<u32, JsonRpcResponse> {
    params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'workspace_id'")
        })
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
    match err {
        TaskNotFound(_) => JsonRpcResponse::error(id, -32004, &err.to_string()),
        DependencyCycle(_)
        | UnknownDependency(_)
        | InvalidArgument(_)
        | InvalidTransition { .. } => JsonRpcResponse::invalid_params(id, err.to_string()),
        AlreadyTerminal(_) => JsonRpcResponse::error(id, -32008, &err.to_string()),
        LeaseConflict { .. } => JsonRpcResponse::error(id, -32009, &err.to_string()),
        Memory(_) | Serde(_) => JsonRpcResponse::error(id, -32603, &err.to_string()),
    }
}

pub(super) fn run_store<F, R>(
    _state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    f: F,
) -> JsonRpcResponse
where
    F: FnOnce(&mut TaskStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let seq = engine.agent_seq.clone();
    let result = with_store(|mem| {
        let mut store = TaskStore::new(mem, tasty_memory::HOST_OWNER, seq.as_ref());
        f(&mut store)
    });
    match result {
        None => JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Ok(v)) => match serde_json::to_value(v) {
            Ok(json) => JsonRpcResponse::success(id, json),
            Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
        },
        Some(Err(e)) => agent_err_to_response(id, e),
    }
}

// ============================================================
// agent.task_create
// ============================================================

pub(super) fn escape_dot(s: &str) -> String {
    s.replace('"', "\\\"").replace('\n', " ")
}

// ============================================================
// agent.barrier_*
// ============================================================

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
mod task;

pub use barrier::*;
pub use lease::*;
pub use ratelimit::*;
pub use semaphore::*;
pub use task::*;
