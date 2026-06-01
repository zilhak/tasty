use serde_json::{Value, json};

use crate::core::Core;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{
    OnFailure, ReducerStrategy, TaskCommand, TaskGraph, TaskId, TaskState, reduce_with_custom,
};

use super::{agent_err_to_response, escape_dot, now_ms, task_id_param, workspace_id_param};

pub fn handle_task_create(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

    let opts = TaskCreateOpts {
        workspace_id,
        name,
        command,
        depends_on,
        on_failure,
        metadata,
        now_ms: now_ms(),
    };
    match core.task_create(engine, opts) {
        Ok(task) => match serde_json::to_value(task) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
        },
        Err(e) => agent_err_to_response(id, e),
    }
}

// ============================================================
// agent.task_list
// ============================================================

pub fn handle_task_list(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

    match core.task_list(engine, workspace_id) {
        Err(e) => agent_err_to_response(id, e),
        Ok(mut tasks) => {
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
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
    match core.task_get(engine, workspace_id, &task_id) {
        Err(e) => agent_err_to_response(id, e),
        Ok(None) => JsonRpcResponse::error(id, -32004, &format!("task not found: {task_id}")),
        Ok(Some(t)) => JsonRpcResponse::success(id, serde_json::to_value(t).unwrap_or(Value::Null)),
    }
}

// ============================================================
// agent.task_cancel
// ============================================================

pub fn handle_task_cancel(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
    match core.task_cancel(engine, workspace_id, &task_id, now_ms()) {
        Err(e) => agent_err_to_response(id, e),
        Ok((task, cascaded)) => JsonRpcResponse::success(
            id,
            json!({
                "task": task,
                "cascaded": cascaded,
            }),
        ),
    }
}

// ============================================================
// agent.task_retry
// ============================================================

pub fn handle_task_retry(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
    match core.task_retry(engine, workspace_id, &task_id, reset_downstream, now_ms()) {
        Err(e) => agent_err_to_response(id, e),
        Ok(task) => match serde_json::to_value(task) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::error(id, -32603, &format!("serialize: {e}")),
        },
    }
}

// ============================================================
// agent.task_await — poll-based 단순 변형 (즉시 응답)
// ============================================================
//
// 본 단계에서는 blocking await 가 아닌 **현재 상태 조회**만 한다. 호출자가
// terminal 상태가 아니면 다시 호출해 폴링한다. 실제 long-poll/wakeup 은
// scheduler 도입 시 별도 구현.
pub fn handle_task_await(
    core: &Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    handle_task_get(core, state, engine, caller, id, params)
}

// ============================================================
// agent.task_graph
// ============================================================

pub fn handle_task_graph(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

    let tasks = match core.task_list(engine, workspace_id) {
        Err(e) => return agent_err_to_response(id, e),
        Ok(t) => t,
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
                    t.depends_on
                        .iter()
                        .map(move |d| json!({"from": d, "to": t.id}))
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

pub fn handle_task_reduce(
    core: &Core,
    _state: &mut AppState,
    engine: &mut crate::core::CoreState,
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

    // 1단계: task 결과 수집 (memory access 안에서). Core 가 lock + store 조립을 담당.
    let collected = match core.task_reduce_collect(engine, workspace_id, &inputs) {
        Err(e) => return agent_err_to_response(id, e),
        Ok(v) => v,
    };

    // 2단계: reducer 실행 (memory lock 바깥에서; custom shell 은 stdin/stdout I/O).
    let result = reduce_with_custom(&strategy, &collected, run_custom_shell);
    match result {
        Ok(value) => JsonRpcResponse::success(id, json!({ "value": value })),
        Err(e) => agent_err_to_response(id, e),
    }
}

// ============================================================
// agent.rate_limit_*  (Phase 5.5)
// ============================================================

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
