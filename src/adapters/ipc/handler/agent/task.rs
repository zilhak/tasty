use serde_json::{Value, json};

use crate::core::Core;
use crate::state::AppState;
use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{
    DispatchHandle, OnFailure, PollSpecRef, ReducerStrategy, TaskCommand, TaskGraph, TaskId,
    TaskResult, TaskState, reduce_with_custom,
};
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

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

    if let Err(e) = validate_poll_strategy_refs(&command, &on_failure) {
        return JsonRpcResponse::invalid_params(id, e);
    }

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
            Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
        },
        Err(e) => agent_err_to_response(id, e),
    }
}

/// `command` 및 `on_failure.Fallback.inline` 이 참조하는 `poll` 이름(완료 판정
/// 전략)을 생성 시점에 검증한다 — task_create 시 미등록 전략 이름을 거부하기
/// 위함이다. 오타를 실행 시점(Custom dispatch)이 아니라 생성 시점에 잡아
/// task 가 Running 에 진입한 뒤에야 실패하는 것을 막는다. `Custom` dispatch 이름
/// 해석(`src/core/agent/runner_host.rs`)과 같은 `resolve_poll_spec` 을 공유한다.
///
/// **범위 주의**: 이 함수는 poll 전략 *이름*만 검증한다. `OnFailure::Fallback.task`
/// 와 `TaskCommand::Reduce.inputs` 가 가리키는 task id 의 존재 검증은 여기가 아니라
/// `core.task_create` → `TaskStore::create`(store 층, `crates/tasty-agent/src/task/
/// store.rs`)가 담당한다 — store 불변식이라 이 handler 를 거치지 않는 호출자에도
/// 적용돼야 하기 때문이다. 두 검증은 서로 다른 거부 사유를 다루므로 겹치지 않는다.
fn validate_poll_strategy_refs(
    command: &TaskCommand,
    on_failure: &OnFailure,
) -> Result<(), String> {
    validate_command_poll_ref(command)?;
    if let OnFailure::Fallback {
        inline: Some(spec), ..
    } = on_failure
    {
        validate_command_poll_ref(&spec.command)?;
        validate_poll_strategy_refs(&spec.command, &spec.on_failure)?;
    }
    Ok(())
}

fn validate_command_poll_ref(command: &TaskCommand) -> Result<(), String> {
    if let TaskCommand::Custom {
        poll: Some(PollSpecRef::Named { strategy }),
        ..
    } = command
    {
        let id = crate::completion_strategy::CompletionStrategyId::new(strategy.clone());
        crate::completion_strategy::global()
            .resolve_poll_spec(&id)
            .map_err(|e| format!("poll strategy '{strategy}': {e}"))?;
    }
    Ok(())
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
                    "runner": runner_status_json(core, engine, workspace_id),
                }),
            )
        }
    }
}

// ============================================================
// 결정 1/5: "runner 가 꺼져 있다" 를 조회로 드러내기 — 새 메서드 없이
// task_list/task_graph 에 runner 상태를 동반하고, task_get 에 hook_wait 대기
// 상태를 동반한다.
// ============================================================

/// `task_run --action status` 와 동일 형태의 runner 상태 요약. runner thread
/// 유무와 무관하게 store 를 직접 조회하므로, runner 가 꺼져 있어도(즉
/// `running: false` 여도) `ready_count`/`running_count` 는 실제 값을 낸다 —
/// "비-terminal task 는 있는데 아무도 안 돌리고 있다"가 `task_list`/`task_graph`
/// 응답만으로 드러나는 이유.
fn runner_status_json(core: &Core, engine: &crate::core::CoreState, workspace_id: u32) -> Value {
    let ctx = core.runner_context(engine);
    let status = core.agent_runner_registry().status(&ctx, workspace_id);
    json!({
        "running": status.running,
        "crashed": status.crashed,
        "ready_count": status.ready_count,
        "running_count": status.running_count,
    })
}

/// task 가 `AwaitExternal` handle 로 외부 신호를 기다리는 중이면 그 사실 +
/// deadline 을 노출한다. `task_get` 이 이 값을 실어야 "그냥 running" 과
/// 구분된다(결정 5) — `AwaitExternal` 의 poll 은 계약상 항상 Active 라 state 만
/// 봐서는 대기 이유를 알 수 없기 때문이다.
fn awaiting_external_json(
    core: &Core,
    engine: &crate::core::CoreState,
    workspace_id: u32,
    task_id: &str,
) -> Option<Value> {
    let ctx = core.runner_context(engine);
    match crate::core::agent::runner_host::load_dispatch_handle(&ctx, workspace_id, task_id)? {
        DispatchHandle::AwaitExternal {
            wait_key,
            deadline_ms,
        } => Some(json!({
            "wait_key": wait_key,
            "deadline_ms": deadline_ms,
        })),
        _ => None,
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
        Ok(None) => JsonRpcResponse::error(id, -32004, format!("task not found: {task_id}")),
        Ok(Some(t)) => {
            let is_running = matches!(t.state, TaskState::Running);
            let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
            // 결정 5: Running 인데 AwaitExternal 로 외부 신호를 기다리는 task 는
            // "그냥 running" 과 구분되게 대기 정보(+deadline)를 함께 싣는다.
            if is_running
                && let Some(obj) = v.as_object_mut()
                && let Some(info) = awaiting_external_json(core, engine, workspace_id, &task_id)
            {
                obj.insert("awaiting_external".to_string(), info);
            }
            JsonRpcResponse::success(id, v)
        }
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
            Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
        },
    }
}

// ============================================================
// agent.task_await — 진짜 blocking (TaskWakerHub 기반)
// ============================================================
//
// J.A.S5: `await_task_blocking` 가 worker thread 안에서 호출되어 (app_methods 의
// `ipc_dispatch_task_await` 분기), 현 state 가 종결이면 즉시 반환, 아니면 hub 에
// waiter 등록 후 recv_timeout. `timeout_ms == None | 0` = 무한 대기 (approval 과
// 다른 정책 — task 는 record-level timeout 없음).
//
// 응답 형식:
//   { outcome: "terminal" | "timed_out" | "not_found",
//     state: "succeeded" | "failed" | ...,  // outcome=terminal 일 때만
//     result: { ... },                        // outcome=terminal 일 때만, 있으면
//   }
pub fn await_task_blocking(
    hub: &crate::core::agent::task_waker::TaskWakerHub,
    memory: &std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
    rpc_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    use crate::core::agent::task_waker::{AwaitOutcome, TerminalSnapshot};
    use tasty_memory::HOST_OWNER;

    let workspace_id = match workspace_id_param(params, &rpc_id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let task_id = match task_id_param(params, &rpc_id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let timeout_ms = params.get("timeout_ms").and_then(|v| v.as_u64());

    // 1. 현 state snapshot.
    let snap_opt: Option<TerminalSnapshot> = {
        let mut guard = match memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let store = tasty_agent::TaskStore::new(&mut *guard, HOST_OWNER, agent_seq.as_ref());
        match store.get(workspace_id, &task_id) {
            Ok(Some(t)) => Some(TerminalSnapshot {
                state: t.state,
                result: t.result,
            }),
            Ok(None) => None,
            Err(_) => None,
        }
    };
    let Some(current) = snap_opt else {
        return JsonRpcResponse::success(rpc_id, json!({ "outcome": "not_found" }));
    };

    // 2. blocking await.
    let outcome = hub.await_terminal(workspace_id, &task_id, timeout_ms, current);
    match outcome {
        AwaitOutcome::Terminal(snap) => {
            let mut resp = json!({
                "outcome": "terminal",
                "state": snap.state.name(),
            });
            if let Some(r) = snap.result {
                resp["result"] = serde_json::to_value(r).unwrap_or(Value::Null);
            }
            JsonRpcResponse::success(rpc_id, resp)
        }
        AwaitOutcome::TimedOut => {
            JsonRpcResponse::success(rpc_id, json!({ "outcome": "timed_out" }))
        }
    }
}

/// 하위 호환을 위한 sync 진입점은 *제거* — handler.rs 의 라우터 분기는 더 이상
/// 본 함수로 보내지 않고, app_methods 의 blocking arm 으로 흐른다. 만약 어떤
/// 경로가 본 함수를 직접 호출하더라도 동작하도록 즉시 응답 fallback 만 제공:
/// 현재 상태를 task_get 처럼 돌려준다.
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
    let runner = runner_status_json(core, engine, workspace_id);

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
                    "runner": runner,
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
                    "runner": runner,
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
// agent.task_run — workspace 단위 runner thread 시작/중단/상태조회
// ============================================================
//
// action: "start" | "stop" | "status".
// 응답: { running, crashed, ready_count, running_count }.
// start: 이미 실행 중이면 no-op (running=true 그대로).
// stop:  실행 중이면 정지 후 join, 아니면 no-op.
// status: 카운트만 갱신.
pub fn handle_task_run(
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
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let ctx = core.runner_context(engine);
    let registry = core.agent_runner_registry();
    match action {
        "start" => {
            registry.start(ctx.clone(), workspace_id);
        }
        "stop" => {
            registry.stop(workspace_id);
        }
        "status" => {}
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("invalid 'action': {other} (expected start|stop|status)"),
            );
        }
    }
    let status = registry.status(&ctx, workspace_id);
    JsonRpcResponse::success(
        id,
        json!({
            "running": status.running,
            "crashed": status.crashed,
            "ready_count": status.ready_count,
            "running_count": status.running_count,
        }),
    )
}

// ============================================================
// agent.task_set_result — 외부 호출자가 task 완료 신호를 보내는 진입점
// ============================================================
//
// runner 가 dispatch 하는 task 외에 *수동 / 외부 통합 task* 의 결과 보고용.
// runner thread 는 Core 의 wrapper 를 직접 호출하므로 본 IPC 를 거치지 않는다.
// state 인자: "succeeded" | "failed" (그 외 거부).
pub fn handle_task_set_result(
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
    let state_str = match params.get("state").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::invalid_params(id, "Missing 'state' ('succeeded' | 'failed')");
        }
    };
    let exit_code = params
        .get("exit_code")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let output = params.get("output").cloned();
    let error = params
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let result = TaskResult {
        exit_code,
        output,
        error: error.clone(),
    };

    // 1단계: result 영속.
    if let Err(e) = core.task_set_result(engine, workspace_id, &task_id, result) {
        return agent_err_to_response(id, e);
    }

    // 2단계: state 전이. "failed" 면 TaskState::Failed { error } 로.
    let new_state = match state_str.as_str() {
        "succeeded" => TaskState::Succeeded,
        "failed" => TaskState::Failed {
            error: error.unwrap_or_else(|| "(unspecified)".to_string()),
        },
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("invalid 'state': {other} (expected 'succeeded' or 'failed')"),
            );
        }
    };

    match core.task_set_state(engine, workspace_id, &task_id, new_state, now_ms()) {
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
// agent.rate_limit_*
// ============================================================

pub(crate) fn run_custom_shell(command: &str, stdin_json: &str) -> std::io::Result<String> {
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

    tasty_utils::process::hide_console(&mut cmd);
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
        return Err(std::io::Error::other(format!(
            "exit_code={}, stderr={}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod poll_strategy_ref_tests {
    use super::*;
    use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

    /// `crate::completion_strategy::global()` 는 프로세스 전역 싱글턴이라 다른 테스트
    /// 파일과 공유된다 — 이 파일이 쓰는 id 는 충돌 방지를 위해 고유 접두어를 쓴다
    /// (`src/completion_strategy/registry_tests.rs` 의 `cstest-*` 관례와 동일 이유).
    fn install_poll_strategy(plugin_id: &str, short_id: &str) {
        crate::completion_strategy::HostCompletionStrategyPort
            .install_plugin_completion_strategies(
                plugin_id,
                &[json!({
                    "id": short_id,
                    "priority": 100,
                    "spec": {
                        "kind": "poll",
                        "poll_method": format!("{plugin_id}.wait"),
                        "state_field": "state",
                        "terminal_states": ["done"],
                    },
                })],
            );
    }

    #[test]
    fn named_poll_ref_to_unregistered_strategy_is_rejected() {
        let command = TaskCommand::Custom {
            ipc_method: "tcpoll1.spawn".into(),
            params: Value::Null,
            poll: Some(PollSpecRef::Named {
                strategy: "tcpoll1/does-not-exist".into(),
            }),
        };
        let err = validate_poll_strategy_refs(&command, &OnFailure::Abort).unwrap_err();
        assert!(err.contains("tcpoll1/does-not-exist"));
    }

    #[test]
    fn named_poll_ref_to_registered_strategy_is_accepted() {
        install_poll_strategy("tcpoll2", "spawn-wait");
        let command = TaskCommand::Custom {
            ipc_method: "tcpoll2.spawn".into(),
            params: Value::Null,
            poll: Some(PollSpecRef::Named {
                strategy: "tcpoll2/spawn-wait".into(),
            }),
        };
        assert!(validate_poll_strategy_refs(&command, &OnFailure::Abort).is_ok());
    }

    #[test]
    fn named_poll_ref_inside_inline_fallback_is_validated() {
        let bad_fallback = TaskCommand::Custom {
            ipc_method: "tcpoll3.spawn".into(),
            params: Value::Null,
            poll: Some(PollSpecRef::Named {
                strategy: "tcpoll3/does-not-exist".into(),
            }),
        };
        let on_failure = OnFailure::Fallback {
            task: None,
            inline: Some(Box::new(tasty_agent::task::InlineFallbackSpec {
                name: "fb".into(),
                command: bad_fallback,
                depends_on_override: None,
                on_failure: OnFailure::Abort,
                metadata: Value::Null,
            })),
        };
        let main_command = TaskCommand::Custom {
            ipc_method: "tcpoll3.main".into(),
            params: Value::Null,
            poll: None,
        };
        let err = validate_poll_strategy_refs(&main_command, &on_failure).unwrap_err();
        assert!(err.contains("tcpoll3/does-not-exist"));
    }

    #[test]
    fn inline_poll_spec_and_no_poll_are_unaffected() {
        let inline_cmd = TaskCommand::Custom {
            ipc_method: "tcpoll4.spawn".into(),
            params: Value::Null,
            poll: Some(PollSpecRef::Inline(tasty_agent::PollSpec {
                poll_method: "tcpoll4.wait".into(),
                map_from_response: Default::default(),
                map_from_request: Default::default(),
                state_field: "state".into(),
                terminal_states: vec!["done".into()],
                interval_ms: 500,
                timeout_ms: None,
            })),
        };
        assert!(validate_poll_strategy_refs(&inline_cmd, &OnFailure::Abort).is_ok());

        let no_poll_cmd = TaskCommand::Custom {
            ipc_method: "tcpoll4.spawn".into(),
            params: Value::Null,
            poll: None,
        };
        assert!(validate_poll_strategy_refs(&no_poll_cmd, &OnFailure::Abort).is_ok());
    }
}
