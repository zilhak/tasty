//! `agent.*` IPC 핸들러 — 협업 primitive (Phase 5).
//!
//! 본 모듈은 `tasty-agent` 의 도메인 모델을 IPC 표면으로 노출한다. 영속은
//! `tasty-memory` 의 workspace scope 에 위임. **task 실행 자체는 본 phase
//! 5.1 범위 밖** — 호스트가 `Ready` task 를 골라 실제 IPC dispatch 를 트리거하는
//! 스케줄러 루프는 후속 5.x 에서 붙는다. 본 단계는 state 머신 / DAG / 영속
//! 정확성만 보장한다.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tasty_agent::{
    AgentError, BarrierStore, LeaseMode, LeaseStore, OnFailure, ReducerInput, ReducerStrategy,
    SemaphoreStore, Task, TaskCommand, TaskGraph, TaskId, TaskState, TaskStore,
    reduce_with_custom,
};
use tasty_memory::with_store;

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn workspace_id_param(params: &Value, id: &Value) -> Result<u32, JsonRpcResponse> {
    params
        .get("workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'workspace_id'")
        })
}

fn task_id_param(params: &Value, id: &Value) -> Result<TaskId, JsonRpcResponse> {
    params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing required 'id' (task id)"))
}

fn agent_err_to_response(id: Value, err: AgentError) -> JsonRpcResponse {
    use AgentError::*;
    match err {
        TaskNotFound(_) => JsonRpcResponse::error(id, -32004, &err.to_string()),
        DependencyCycle(_) | UnknownDependency(_) | InvalidArgument(_) | InvalidTransition { .. } => {
            JsonRpcResponse::invalid_params(id, err.to_string())
        }
        AlreadyTerminal(_) => JsonRpcResponse::error(id, -32008, &err.to_string()),
        LeaseConflict { .. } => JsonRpcResponse::error(id, -32009, &err.to_string()),
        Memory(_) | Serde(_) => JsonRpcResponse::error(id, -32603, &err.to_string()),
    }
}

fn run_store<F, R>(state: &mut AppState, id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut TaskStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let seq = state.engine.agent_seq.clone();
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

pub fn handle_task_create(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing required 'name'"),
    };
    let command: TaskCommand = match params.get("command") {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => {
                return JsonRpcResponse::invalid_params(id, format!("invalid 'command': {e}"));
            }
        },
        None => return JsonRpcResponse::invalid_params(id, "Missing required 'command'"),
    };
    let depends_on: Vec<TaskId> = params
        .get("depends_on")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let on_failure: OnFailure = params
        .get("on_failure")
        .map(|v| serde_json::from_value(v.clone()).unwrap_or_default())
        .unwrap_or_default();
    let metadata = params.get("metadata").cloned().unwrap_or(Value::Null);

    let ts = now_ms();
    run_store(state, id, move |store| {
        store.create(workspace_id, name, command, depends_on, on_failure, metadata, ts)
    })
}

// ============================================================
// agent.task_list
// ============================================================

pub fn handle_task_list(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let state_filter = params
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let seq = state.engine.agent_seq.clone();
    let result: Option<Result<Vec<Task>, AgentError>> = with_store(|mem| {
        let store = TaskStore::new(mem, tasty_memory::HOST_OWNER, seq.as_ref());
        store.list(workspace_id)
    });
    match result {
        None => JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Err(e)) => agent_err_to_response(id, e),
        Some(Ok(mut tasks)) => {
            if let Some(filter) = state_filter {
                tasks.retain(|t| t.state.name() == filter);
            }
            JsonRpcResponse::success(
                id,
                json!({
                    "total": tasks.len(),
                    "tasks": tasks,
                }),
            )
        }
    }
}

// ============================================================
// agent.task_get
// ============================================================

pub fn handle_task_get(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let task_id = match task_id_param(params, &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let seq = state.engine.agent_seq.clone();
    let result = with_store(|mem| {
        let store = TaskStore::new(mem, tasty_memory::HOST_OWNER, seq.as_ref());
        store.get(workspace_id, &task_id)
    });
    match result {
        None => JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Err(e)) => agent_err_to_response(id, e),
        Some(Ok(None)) => JsonRpcResponse::error(id, -32004, &format!("task not found: {task_id}")),
        Some(Ok(Some(t))) => JsonRpcResponse::success(id, serde_json::to_value(t).unwrap_or(Value::Null)),
    }
}

// ============================================================
// agent.task_cancel
// ============================================================

pub fn handle_task_cancel(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let task_id = match task_id_param(params, &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let ts = now_ms();
    run_store(state, id, move |store| {
        let (task, cascaded) = store.cancel(workspace_id, &task_id, ts)?;
        Ok::<_, AgentError>(json!({
            "task": task,
            "cascaded": cascaded,
        }))
    })
}

// ============================================================
// agent.task_retry
// ============================================================

pub fn handle_task_retry(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let task_id = match task_id_param(params, &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let reset_downstream = params
        .get("reset_downstream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ts = now_ms();
    run_store(state, id, move |store| {
        store.retry(workspace_id, &task_id, reset_downstream, ts)
    })
}

// ============================================================
// agent.task_await — poll-based 단순 변형 (즉시 응답)
// ============================================================
//
// 본 단계에서는 blocking await 가 아닌 **현재 상태 조회**만 한다. 호출자가
// terminal 상태가 아니면 다시 호출해 폴링한다. 실제 long-poll/wakeup 은
// scheduler 도입 시 별도 구현.
pub fn handle_task_await(
    state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_task_get(state, caller, id, params)
}

// ============================================================
// agent.task_graph
// ============================================================

pub fn handle_task_graph(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json")
        .to_string();

    let seq = state.engine.agent_seq.clone();
    let result = with_store(|mem| {
        let store = TaskStore::new(mem, tasty_memory::HOST_OWNER, seq.as_ref());
        store.list(workspace_id)
    });
    let tasks = match result {
        None => return JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Err(e)) => return agent_err_to_response(id, e),
        Some(Ok(t)) => t,
    };

    // 사이클은 detection 만 — graph 그리기는 그대로 한다.
    let cycle = TaskGraph::build(&tasks).detect_cycles().err();

    match format.as_str() {
        "dot" => {
            let mut out = String::from("digraph G {\n  rankdir=LR;\n");
            for t in &tasks {
                let color = match &t.state {
                    TaskState::Ready => "lightblue",
                    TaskState::Running => "yellow",
                    TaskState::Succeeded => "lightgreen",
                    TaskState::Failed { .. } => "salmon",
                    TaskState::Cancelled => "gray",
                    TaskState::Skipped => "lightgray",
                    TaskState::Waiting => "white",
                    TaskState::Unknown => "orange",
                };
                out.push_str(&format!(
                    "  \"{}\" [label=\"{}\\n{}\", style=filled, fillcolor={}];\n",
                    t.id,
                    escape_dot(&t.name),
                    t.state.name(),
                    color
                ));
            }
            for t in &tasks {
                for dep in &t.depends_on {
                    out.push_str(&format!("  \"{}\" -> \"{}\";\n", dep, t.id));
                }
            }
            out.push_str("}\n");
            JsonRpcResponse::success(
                id,
                json!({
                    "format": "dot",
                    "dot": out,
                    "cycle": cycle.as_ref().map(|e| e.to_string()),
                }),
            )
        }
        _ => {
            let nodes: Vec<Value> = tasks
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "name": t.name,
                        "state": t.state.name(),
                    })
                })
                .collect();
            let edges: Vec<Value> = tasks
                .iter()
                .flat_map(|t| {
                    t.depends_on.iter().map(move |d| {
                        json!({"from": d, "to": t.id})
                    })
                })
                .collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "format": "json",
                    "nodes": nodes,
                    "edges": edges,
                    "cycle": cycle.as_ref().map(|e| e.to_string()),
                }),
            )
        }
    }
}

fn escape_dot(s: &str) -> String {
    s.replace('"', "\\\"").replace('\n', " ")
}

// ============================================================
// agent.barrier_*
// ============================================================

fn name_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing required 'name'"))
}

fn run_barrier<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut BarrierStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = BarrierStore::new(mem, tasty_memory::HOST_OWNER);
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

pub fn handle_barrier_create(
    _state: &mut AppState,
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
    let count_required = match params.get("count_required").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'count_required' (must be u32 >= 1)",
            );
        }
    };
    let timeout_ms = params.get("timeout_ms").and_then(|v| v.as_u64());
    let now = now_ms();
    run_barrier(id, move |store| {
        store.create(workspace_id, name, count_required, timeout_ms, now)
    })
}

pub fn handle_barrier_signal(
    _state: &mut AppState,
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
    let now = now_ms();
    run_barrier(id, move |store| store.signal(workspace_id, &name, now))
}

pub fn handle_barrier_state(
    _state: &mut AppState,
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
    let now = now_ms();
    run_barrier(id, move |store| store.state(workspace_id, &name, now))
}

/// Phase 5.2 단계: poll-based — 상태 조회와 동일. 추후 blocking + wakeup 도입.
pub fn handle_barrier_await(
    state: &mut AppState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_barrier_state(state, caller, id, params)
}

// ============================================================
// agent.semaphore_*
// ============================================================

fn run_semaphore<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut SemaphoreStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = SemaphoreStore::new(mem, tasty_memory::HOST_OWNER);
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

pub fn handle_semaphore_create(
    _state: &mut AppState,
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
    let permits = match params.get("permits").and_then(|v| v.as_u64()) {
        Some(c) if c <= u32::MAX as u64 => c as u32,
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or invalid 'permits' (must be u32 >= 1)",
            );
        }
    };
    let now = now_ms();
    run_semaphore(id, move |store| {
        store.create(workspace_id, name, permits, now)
    })
}

pub fn handle_semaphore_acquire(
    _state: &mut AppState,
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
    run_semaphore(id, move |store| {
        store.acquire(workspace_id, &name, &holder)
    })
}

pub fn handle_semaphore_release(
    _state: &mut AppState,
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
    run_semaphore(id, move |store| {
        store.release(workspace_id, &name, &holder)
    })
}

// ============================================================
// agent.lease_*
// ============================================================

fn run_lease<F, R>(id: Value, f: F) -> JsonRpcResponse
where
    F: FnOnce(&mut LeaseStore<'_>) -> Result<R, AgentError>,
    R: serde::Serialize,
{
    let result = with_store(|mem| {
        let mut store = LeaseStore::new(mem, tasty_memory::HOST_OWNER);
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

fn resource_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("resource")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing or empty 'resource'"))
}

fn holder_param(params: &Value, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get("holder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing or empty 'holder'"))
}

pub fn handle_lease_acquire(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let resource = match resource_param(params, &id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let holder = match holder_param(params, &id) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let ttl_ms = params.get("ttl_ms").and_then(|v| v.as_u64());
    let mode = match params.get("mode").and_then(|v| v.as_str()).unwrap_or("fail") {
        "fail" => LeaseMode::Fail,
        "block" => LeaseMode::Block,
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("'mode' must be 'fail' or 'block', got '{other}'"),
            );
        }
    };
    let now = now_ms();
    run_lease(id, move |store| {
        store.acquire(workspace_id, &resource, &holder, ttl_ms, mode, now)
    })
}

pub fn handle_lease_release(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let resource = match resource_param(params, &id) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let holder = match holder_param(params, &id) {
        Ok(h) => h,
        Err(e) => return e,
    };
    run_lease(id, move |store| {
        store.release(workspace_id, &resource, &holder)
    })
}

pub fn handle_lease_list(
    _state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let now = now_ms();
    run_lease(id, move |store| {
        let leases = store.list(workspace_id, Some(now))?;
        Ok(json!({ "total": leases.len(), "leases": leases }))
    })
}

// ============================================================
// agent.task_reduce — 다른 task 결과 합성
// ============================================================

pub fn handle_task_reduce(
    state: &mut AppState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let inputs: Vec<TaskId> = match params.get("inputs").and_then(|v| v.as_array()) {
        Some(arr) if !arr.is_empty() => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => {
            return JsonRpcResponse::invalid_params(
                id,
                "Missing or empty 'inputs' (array of task ids)",
            );
        }
    };
    let strategy_val = match params.get("strategy") {
        Some(v) => v.clone(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'strategy'"),
    };
    let strategy: ReducerStrategy = match serde_json::from_value(strategy_val) {
        Ok(s) => s,
        Err(e) => {
            return JsonRpcResponse::invalid_params(id, format!("invalid 'strategy': {e}"));
        }
    };

    // 1단계: task 결과 수집 (memory access 안에서).
    let seq = state.engine.agent_seq.clone();
    let collected: Option<Result<Vec<ReducerInput>, AgentError>> = with_store(|mem| {
        let store = TaskStore::new(mem, tasty_memory::HOST_OWNER, seq.as_ref());
        let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
        for tid in &inputs {
            let task = match store.get(workspace_id, tid) {
                Ok(Some(t)) => t,
                Ok(None) => return Err(AgentError::TaskNotFound(tid.clone())),
                Err(e) => return Err(e),
            };
            let succeeded = matches!(task.state, TaskState::Succeeded);
            let output = task
                .result
                .and_then(|r| r.output)
                .unwrap_or(Value::Null);
            out.push(ReducerInput { succeeded, output });
        }
        Ok(out)
    });
    let collected = match collected {
        None => return JsonRpcResponse::error(id, -32603, "memory store not initialized"),
        Some(Err(e)) => return agent_err_to_response(id, e),
        Some(Ok(v)) => v,
    };

    // 2단계: reducer 실행 (memory lock 바깥에서; custom shell 은 stdin/stdout I/O).
    let result = reduce_with_custom(&strategy, &collected, run_custom_shell);
    match result {
        Ok(value) => JsonRpcResponse::success(id, json!({ "value": value })),
        Err(e) => agent_err_to_response(id, e),
    }
}

/// `Custom { command }` 실행기. `command` 를 시스템 셸로 실행하고 stdin 으로
/// JSON 배열을 흘려보낸 뒤 stdout 을 수확한다. Windows 는 `cmd /C`, 그 외는
/// `sh -c`.
fn run_custom_shell(command: &str, stdin_json: &str) -> std::io::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut sin) = child.stdin.take() {
        sin.write_all(stdin_json.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "exit_code={}, stderr={}",
                out.status.code().unwrap_or(-1),
                stderr.trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
