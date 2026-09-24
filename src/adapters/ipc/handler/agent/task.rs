use std::collections::BTreeSet;

use crate::adapters::ipc::handler::params::{self, p_try};
use serde_json::{Value, json};

use crate::core::Core;
use crate::core::agent::graph_view::{collect_graph_edges, on_failure_kind, task_command_kind};
use tasty_agent::task::{TaskCreateOpts, TaskDeleteOpts, TaskPurgeFilter};
use tasty_agent::{
    AgentError, DispatchHandle, OnFailure, PollSpecRef, ReducerStrategy, Task, TaskCommand,
    TaskGraph, TaskId, TaskResult, TaskState, extract_paths, reduce_with_custom, run_custom_shell,
};
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::super::memory::mark_durability;
use super::{agent_err_to_response, escape_dot, now_ms, task_id_param, workspace_id_param};

pub fn handle_task_create(
    core: &Core,
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
    // 다른 작업의 fallback으로 등록하기 전 실행되지 않도록 Waiting 상태로 예약한다.
    let reserved_for_fallback = params
        .get("reserved_for_fallback")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if let Err(e) = validate_poll_strategy_refs(&command, &on_failure) {
        return JsonRpcResponse::invalid_params(id, e);
    }
    if let Err(e) = validate_task_output_refs(&command, &depends_on, &on_failure) {
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
    mark_durability(
        core,
        match core.task_create(engine, opts, reserved_for_fallback) {
            Ok(task) => match serde_json::to_value(&task) {
                Ok(mut v) => {
                    if let Some(warnings) = fallback_with_deps_warning(&task)
                        && let Some(obj) = v.as_object_mut()
                    {
                        obj.insert("warnings".into(), json!(warnings));
                    }
                    JsonRpcResponse::success(id, v)
                }
                Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
            },
            Err(e) => agent_err_to_response(id, e),
        },
    )
}

/// Fallback은 작업 자체의 실행 실패에만 적용된다. 의존성이 실패하면 자기 fallback을
/// 실행하지 않고 Waiting에 남으므로, 생성은 허용하되 이 차이를 경고한다.
fn fallback_with_deps_warning(task: &tasty_agent::Task) -> Option<Vec<&'static str>> {
    if matches!(task.on_failure, OnFailure::Fallback { .. }) && !task.depends_on.is_empty() {
        Some(vec![
            "on_failure=Fallback only takes effect when this task itself fails (Running -> Failed); \
             it has no effect on the Waiting -> Skipped transition caused by a failed dependency (depends_on)",
        ])
    } else {
        None
    }
}

/// 실행을 시작하기 전에 poll 전략 이름을 검증한다. 인라인 fallback에도 적용한다.
/// 다른 작업 ID의 존재 여부는 핸들러를 거치지 않는 호출에도 적용되도록 TaskStore에서 검사한다.
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

/// 출력 참조의 작업이 선언된 의존성에 있어야 결과가 준비된 뒤 실행할 수 있다.
/// 의존성을 자동 추가하지 않고 누락을 거절한다. 문법 검사도 실행 시 치환과 같은 파서를 쓴다.
fn validate_task_output_refs(
    command: &TaskCommand,
    depends_on: &[TaskId],
    on_failure: &OnFailure,
) -> Result<(), String> {
    use crate::core::agent::task_output_ref;

    let mut available: BTreeSet<&str> = depends_on.iter().map(String::as_str).collect();
    if let TaskCommand::Reduce { inputs, .. } = command {
        available.extend(inputs.iter().map(String::as_str));
    }
    for tid in task_output_ref::referenced_tasks(command).map_err(|e| e.0)? {
        if !available.contains(tid.as_str()) {
            return Err(format!(
                "task output reference '{tid}' is not in depends_on; add it so this task runs after '{tid}' finishes"
            ));
        }
    }
    // 인라인 fallback은 별도 작업이다. override가 없으면 본 작업의 의존성을 물려받는다.
    if let OnFailure::Fallback {
        inline: Some(spec), ..
    } = on_failure
    {
        let inherited: Vec<TaskId> = spec
            .depends_on_override
            .clone()
            .unwrap_or_else(|| depends_on.to_vec());
        validate_task_output_refs(&spec.command, &inherited, &spec.on_failure)?;
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

/// 배열·콤마 구분 문자열·단일 문자열을 받고 state/states를 서로 별칭으로 쓴다.
/// 조회에서 빈 값은 필터 없음이다. 삭제는 빈 값을 보존하는 state_names_param_keep_empty를 쓴다.
fn state_names_param(params: &Value, primary_key: &str) -> Option<Vec<String>> {
    let names = state_names_param_keep_empty(params, primary_key)?;
    (!names.is_empty()).then_some(names)
}

/// 키 부재나 null은 필터 없음, 빈 배열·문자열은 매칭 없음으로 구분한다.
/// 잘못된 타입도 매칭 없음으로 처리한다. 빈 선택을 전체 삭제로 해석하지 않기 위해서다.
fn state_names_param_keep_empty(params: &Value, primary_key: &str) -> Option<Vec<String>> {
    let alias = if primary_key == "states" {
        "state"
    } else {
        "states"
    };
    let raw = params
        .get(primary_key)
        .or_else(|| params.get(alias))
        .filter(|v| !v.is_null())?;

    let names: Vec<String> = match raw {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|x| x.as_str())
            .flat_map(split_state_names)
            .collect(),
        Value::String(s) => split_state_names(s.as_str()),
        _ => Vec::new(),
    };
    Some(names)
}

fn split_state_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// state 이름 목록으로 task 를 걸러낸다. `None` 이면 필터 없음(전체 유지).
fn retain_by_state(tasks: &mut Vec<Task>, states: Option<&[String]>) {
    let Some(states) = states else { return };
    tasks.retain(|t| states.iter().any(|s| s == t.state.name()));
}

pub fn handle_task_list(
    core: &Core,
    engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let state_filter = state_names_param(params, "state");

    match core.task_list(engine, workspace_id) {
        Err(e) => agent_err_to_response(id, e),
        Ok(mut tasks) => {
            retain_by_state(&mut tasks, state_filter.as_deref());
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

/// 러너가 꺼져 있어도 저장소를 조회해 실제 작업 수를 반환한다.
/// 조회 실패 시 카운트는 0이 아니라 null이며 store_error에 원인이 담긴다.
/// list_failures는 러너의 연속 조회 실패 횟수다. 스레드가 살아 있어도 작업이 진행되지 않을 수 있다.
fn runner_status_json(core: &Core, engine: &crate::core::CoreState, workspace_id: u32) -> Value {
    let ctx = core.runner_context(engine);
    let status = core.agent_runner_registry().status(&ctx, workspace_id);
    runner_status_value(&status)
}

fn runner_status_value(status: &crate::core::agent::runner_thread::RunnerStatus) -> Value {
    json!({
        "running": status.running,
        "crashed": status.crashed,
        "ready_count": status.ready_count,
        "running_count": status.running_count,
        "store_error": status.store_error,
        "list_failures": status.list_failures,
    })
}

/// AwaitExternal은 상태만 보면 Running이므로 대기 중인 신호와 기한을 함께 반환한다.
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

pub fn handle_task_get(
    core: &Core,
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

pub fn handle_task_cancel(
    core: &Core,
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
    mark_durability(
        core,
        match core.task_cancel(engine, workspace_id, &task_id, now_ms()) {
            Err(e) => agent_err_to_response(id, e),
            Ok((task, cascaded)) => JsonRpcResponse::success(
                id,
                json!({
                    "task": task,
                    "cascaded": cascaded,
                }),
            ),
        },
    )
}

pub fn handle_task_retry(
    core: &Core,
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
    mark_durability(
        core,
        match core.task_retry(engine, workspace_id, &task_id, reset_downstream, now_ms()) {
            Err(e) => agent_err_to_response(id, e),
            Ok(task) => match serde_json::to_value(task) {
                Ok(v) => JsonRpcResponse::success(id, v),
                Err(e) => JsonRpcResponse::error(id, -32603, format!("serialize: {e}")),
            },
        },
    )
}

/// 생략 시 기본 대기 시간은 10분이다. 측정으로 정한 값은 아니므로 사용 경험에 따라 재검토한다.
/// timeout_ms를 명시적으로 0으로 지정하면 무한 대기한다.
pub const DEFAULT_TASK_AWAIT_TIMEOUT_MS: u64 = 600_000;

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
    let timeout_ms = Some(
        p_try!(params::opt_int::<u64>(params, "timeout_ms", &rpc_id))
            .unwrap_or(DEFAULT_TASK_AWAIT_TIMEOUT_MS),
    );

    let snap_opt: Option<TerminalSnapshot> = {
        let mut guard = crate::poison::recover_mutex(
            memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
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

/// 메인 루프를 막지 않도록 워커에서 완료를 기다린다.
/// GUI와 헤드리스의 engine 선택 방식이 달라, 호출자가 대상 저장소를 먼저 고른다.
pub(crate) fn spawn_task_await(
    hub: std::sync::Arc<crate::core::agent::task_waker::TaskWakerHub>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    agent_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
    rpc_id: Value,
    params: Value,
    response_tx: &std::sync::mpsc::SyncSender<JsonRpcResponse>,
) {
    let response_tx = response_tx.clone();
    std::thread::spawn(move || {
        let resp = await_task_blocking(&hub, &memory, agent_seq, rpc_id, &params);
        tasty_ipc::server::send_response(&response_tx, resp);
    });
}

/// task_graph와 dag_get의 공통 dot 렌더. 엣지 수집은 json과 collect_graph_edges를 공유한다.
/// depends_on/Reduce.inputs는 의존성 엣지다. fallback도 표시하지만 사이클 검사 대상은 아니다.
fn render_graph_dot(tasks: &[Task]) -> String {
    let mut out = String::from("digraph G {\n  rankdir=LR;\n");
    for t in tasks {
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
    for edge in collect_graph_edges(tasks) {
        let (style, color) = match edge.kind {
            "fallback" => ("dashed", "orangered"),
            "reduce" => ("dotted", "blue"),
            _ => {
                out.push_str(&format!("  \"{}\" -> \"{}\";\n", edge.from, edge.to));
                continue;
            }
        };
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [style={}, color={}, label=\"{}\"];\n",
            edge.from, edge.to, style, color, edge.kind
        ));
    }
    out.push_str("}\n");
    out
}

fn render_graph_nodes(tasks: &[Task]) -> Vec<Value> {
    tasks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "state": t.state.name(),
                "command_kind": task_command_kind(&t.command),
                "on_failure_kind": on_failure_kind(&t.on_failure),
            })
        })
        .collect()
}

/// depends_on, Reduce.inputs, Fallback.task와 인라인 fallback의 역참조를 구분해 표시한다.
/// fallback 순환은 사이클 검사 대상이 아니므로 표시만 하며 생성 시 거절하지 않는다.
fn render_graph_edges(tasks: &[Task]) -> Vec<Value> {
    collect_graph_edges(tasks)
        .into_iter()
        .map(|edge| json!({"from": edge.from, "to": edge.to, "kind": edge.kind}))
        .collect()
}

pub fn handle_task_graph(
    core: &Core,
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

    // 사이클이 있어도 그래프는 반환한다.
    let cycle = TaskGraph::build(&tasks).detect_cycles().err();
    let runner = runner_status_json(core, engine, workspace_id);

    match format.as_str() {
        "dot" => JsonRpcResponse::success(
            id,
            json!({
                "format": "dot",
                "dot": render_graph_dot(&tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
        _ => {
            let nodes: Vec<Value> = render_graph_nodes(&tasks);
            let edges: Vec<Value> = render_graph_edges(&tasks);
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

/// workspace_id를 생략하면 현재 모든 workspace를 조회한다. 잘못된 타입은 거절한다.
fn optional_workspace_id_param(params: &Value, id: &Value) -> Result<Option<u32>, JsonRpcResponse> {
    crate::adapters::ipc::handler::params::optional_u32(params, "workspace_id", id)
}

/// 목록 조회의 전송량을 줄이기 위해 요청한 경우에만 task_ids를 포함한다.
fn dag_summary_json(dag: &tasty_agent::DagSummary, include_tasks: bool) -> Value {
    let mut v = serde_json::to_value(dag).unwrap_or(Value::Null);
    if !include_tasks && let Some(obj) = v.as_object_mut() {
        obj.remove("task_ids");
    }
    v
}

pub fn handle_dag_list(
    core: &Core,
    engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match optional_workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let include_tasks = params
        .get("include_tasks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    match core.dag_list(engine, workspace_id) {
        Err(e) => agent_err_to_response(id, e),
        Ok(dags) => {
            let rendered: Vec<Value> = dags
                .iter()
                .map(|d| dag_summary_json(d, include_tasks))
                .collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "total": rendered.len(),
                    "dags": rendered,
                    // 삭제된 workspace에 남은 작업은 이 목록에 포함되지 않는다.
                    "scope": "live_workspaces",
                }),
            )
        }
    }
}

/// 명시적으로 묶은 DAG는 다른 그룹의 작업에 의존할 수 있다.
/// 이 경우 UnknownDependency를 사이클로 보고하지 않는다. DagSummary::has_cycle과 같은 기준이다.
/// workspace 전체 조회에서는 실제 끊어진 참조이므로 이 예외를 적용하지 않는다.
fn subset_cycle(tasks: &[Task]) -> Option<AgentError> {
    match TaskGraph::build(tasks).detect_cycles() {
        Err(e @ AgentError::DependencyCycle(_)) => Some(e),
        _ => None,
    }
}

pub fn handle_dag_get(
    core: &Core,
    engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match optional_workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let dag_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing required 'id' (dag id)"),
    };
    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json")
        .to_string();

    let (dag, tasks) = match core.dag_get(engine, workspace_id, &dag_id) {
        Err(e) => return agent_err_to_response(id, e),
        Ok(None) => {
            return JsonRpcResponse::invalid_params(id, format!("unknown dag id: {dag_id}"));
        }
        Ok(Some(found)) => found,
    };

    let cycle = subset_cycle(&tasks);
    let runner = runner_status_json(core, engine, dag.workspace_id);
    let summary = dag_summary_json(&dag, true);

    match format.as_str() {
        "dot" => JsonRpcResponse::success(
            id,
            json!({
                "format": "dot",
                "dag": summary,
                "dot": render_graph_dot(&tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
        _ => JsonRpcResponse::success(
            id,
            json!({
                "format": "json",
                "dag": summary,
                "nodes": render_graph_nodes(&tasks),
                "edges": render_graph_edges(&tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
    }
}

pub fn handle_task_reduce(
    core: &Core,
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
    let extract_path = params.get("extract_path").and_then(|v| v.as_str());

    let collected = match core.task_reduce_collect(engine, workspace_id, &inputs) {
        Err(e) => return agent_err_to_response(id, e),
        Ok(v) => v,
    };

    // 경로가 없는 입력도 버리지 않고 null과 경고로 남긴 뒤 나머지를 합친다.
    let (collected, warnings) = extract_paths(&collected, extract_path);

    // custom shell의 입출력 동안 저장소 잠금을 잡지 않는다.
    let result = reduce_with_custom(&strategy, &collected, run_custom_shell);
    match result {
        Ok(value) => JsonRpcResponse::success(id, json!({ "value": value, "warnings": warnings })),
        Err(e) => agent_err_to_response(id, e),
    }
}

/// workspace 러너를 시작·중지하거나 상태를 조회한다. 이미 시작/중지된 경우는 그대로 둔다.
/// 상태 조회 실패 시 카운트는 null이고 store_error에 원인을 담는다.
pub fn handle_task_run(
    core: &Core,
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
    JsonRpcResponse::success(id, runner_status_value(&status))
}

/// 외부에서 작업 결과를 보고하는 진입점. 러너는 이 IPC 대신 Core를 직접 호출한다.
pub fn handle_task_set_result(
    core: &Core,
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
    let exit_code = p_try!(
        params::read_signed::<i32>(params, "exit_code")
            .map_err(|msg| JsonRpcResponse::invalid_params(id.clone(), msg))
    );
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

    if let Err(e) = core.task_set_result(engine, workspace_id, &task_id, result) {
        return agent_err_to_response(id, e);
    }

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

    mark_durability(
        core,
        match core.task_set_state(engine, workspace_id, &task_id, new_state, now_ms()) {
            Err(e) => agent_err_to_response(id, e),
            Ok((task, cascaded)) => JsonRpcResponse::success(
                id,
                json!({
                    "task": task,
                    "cascaded": cascaded,
                }),
            ),
        },
    )
}

/// 참조가 있으면 거절하고 error.data.referenced_by에 참조자를 반환한다.
/// cascade는 참조자를 함께 지우고 force는 참조 검사만 생략한다. Running 삭제는 항상 거절한다.
pub fn handle_task_delete(
    core: &Core,
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
    let cascade = params
        .get("cascade")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let force = params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    mark_durability(
        core,
        match core.task_delete(
            engine,
            workspace_id,
            &task_id,
            TaskDeleteOpts { cascade, force },
        ) {
            Err(e) => agent_err_to_response(id, e),
            Ok(report) => JsonRpcResponse::success(id, json!({ "deleted": report.deleted })),
        },
    )
}

/// 빈 states는 매칭 없음으로 보존한다. 잘못된 older_than_ms는 거절한다.
/// 두 값을 필터 없음으로 처리하면 의도보다 넓은 범위를 지울 수 있다.
fn purge_filter_from_params(params: &Value, now_ms: u64) -> Result<TaskPurgeFilter, String> {
    Ok(TaskPurgeFilter {
        states: state_names_param_keep_empty(params, "states"),
        older_than_ms: params::read_int::<u64>(params, "older_than_ms")?,
        now_ms,
    })
}

/// 상태·경과시간 필터로 고른 작업 중 참조 검사와 Running 제외 조건을 만족하는 것만 지운다.
/// 두 필터를 모두 생략하면 거절한다. dry_run은 deleted/retained 계획만 반환한다.
pub fn handle_task_purge(
    core: &Core,
    engine: &mut crate::core::CoreState,
    _caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let workspace_id = match workspace_id_param(params, &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let filter = match purge_filter_from_params(params, now_ms()) {
        Ok(f) => f,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    };
    mark_durability(
        core,
        match core.task_purge(engine, workspace_id, filter, dry_run) {
            Err(e) => agent_err_to_response(id, e),
            Ok(plan) => JsonRpcResponse::success(
                id,
                json!({
                    "deleted": plan.deleted,
                    "retained": plan.retained,
                    "dry_run": dry_run,
                }),
            ),
        },
    )
}

#[cfg(test)]
mod poll_strategy_ref_tests {
    use super::*;
    use tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort;

    // 전역 레지스트리를 다른 시험과 공유하므로 고유한 ID 접두어를 쓴다.
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

    fn tell_with(params: serde_json::Value) -> TaskCommand {
        TaskCommand::Custom {
            ipc_method: "claude.tell".into(),
            params,
            poll: None,
        }
    }

    #[test]
    fn task_create_rejects_output_ref_not_in_depends_on() {
        let command = tell_with(json!({
            "surface_id": "${task.t-a.output/child_surface_id}",
        }));
        let err = validate_task_output_refs(&command, &[], &OnFailure::Abort).unwrap_err();
        assert!(err.contains("t-a"), "{err}");
        assert!(err.contains("depends_on"), "{err}");
    }

    #[test]
    fn task_create_accepts_output_ref_declared_in_depends_on() {
        let command = tell_with(json!({
            "surface_id": "${task.t-a.output/child_surface_id}",
        }));
        assert!(
            validate_task_output_refs(&command, &["t-a".to_string()], &OnFailure::Abort).is_ok()
        );
    }

    #[test]
    fn task_create_rejects_when_any_referenced_task_is_undeclared() {
        let command = tell_with(json!({
            "a": "${task.t-a.output/x}",
            "b": "${task.t-b.output/y}",
        }));
        let err = validate_task_output_refs(&command, &["t-a".to_string()], &OnFailure::Abort)
            .unwrap_err();
        assert!(err.contains("t-b"), "{err}");
    }

    #[test]
    fn task_create_accepts_output_ref_from_reduce_inputs() {
        // Reduce에는 치환할 값이 없지만 inputs를 의존성으로 허용하는지는 확인한다.
        let command = TaskCommand::Reduce {
            inputs: vec!["t-a".to_string()],
            strategy: tasty_agent::ReducerStrategy::ConcatText,
        };
        assert!(validate_task_output_refs(&command, &[], &OnFailure::Abort).is_ok());
    }

    #[test]
    fn task_create_validates_output_refs_inside_inline_fallback() {
        let inline = tasty_agent::InlineFallbackSpec {
            name: "fb".into(),
            command: tell_with(json!({ "surface_id": "${task.t-ghost.output/id}" })),
            depends_on_override: None,
            on_failure: OnFailure::Abort,
            metadata: Value::Null,
        };
        let on_failure = OnFailure::Fallback {
            task: None,
            inline: Some(Box::new(inline.clone())),
        };
        let err =
            validate_task_output_refs(&tell_with(Value::Null), &["t-a".to_string()], &on_failure)
                .unwrap_err();
        assert!(err.contains("t-ghost"), "{err}");

        let ok_inline = tasty_agent::InlineFallbackSpec {
            depends_on_override: Some(vec!["t-ghost".to_string()]),
            ..inline
        };
        assert!(
            validate_task_output_refs(
                &tell_with(Value::Null),
                &[],
                &OnFailure::Fallback {
                    task: None,
                    inline: Some(Box::new(ok_inline)),
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn task_create_rejects_malformed_output_placeholder() {
        let command = tell_with(json!({ "x": "${task.t-a.ouput/id}" }));
        let err = validate_task_output_refs(&command, &["t-a".to_string()], &OnFailure::Abort)
            .unwrap_err();
        assert!(err.contains("malformed"), "{err}");
    }

    #[test]
    fn task_create_ignores_commands_without_placeholders() {
        let command = tell_with(json!({ "surface_id": 3, "message": "hi" }));
        assert!(validate_task_output_refs(&command, &[], &OnFailure::Abort).is_ok());
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
                failure_states: vec![],
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

#[cfg(test)]
mod graph_edge_tests {
    use super::*;
    use crate::core::agent::graph_view::GraphEdge;

    fn task(id: &str, name: &str, state: TaskState) -> Task {
        Task {
            id: id.to_string(),
            workspace_id: 1,
            name: name.to_string(),
            command: TaskCommand::Run {
                command: vec!["true".to_string()],
                workspace_id: 1,
                cwd: None,
            },
            depends_on: Vec::new(),
            state,
            created_at: 0,
            started_at: None,
            finished_at: None,
            result: None,
            on_failure: OnFailure::Abort,
            metadata: Value::Null,
            reserved_for_fallback: false,
        }
    }

    // 그룹 밖 의존성은 DAG의 사이클이 아니다. has_cycle과 cycle 응답이 일치해야 한다.
    #[test]
    fn subset_cycle_ignores_dependency_pointing_outside_the_subset() {
        let mut inside = task("t-in", "in", TaskState::Waiting);
        inside.depends_on = vec!["t-out".to_string()];
        let subset = [inside];
        assert!(matches!(
            TaskGraph::build(&subset).detect_cycles(),
            Err(AgentError::UnknownDependency(_))
        ));
        assert!(subset_cycle(&subset).is_none());
    }

    #[test]
    fn subset_cycle_reports_a_real_cycle() {
        let mut a = task("t-a", "a", TaskState::Waiting);
        a.depends_on = vec!["t-b".to_string()];
        let mut b = task("t-b", "b", TaskState::Waiting);
        b.depends_on = vec!["t-a".to_string()];
        let cycle = subset_cycle(&[a, b]).expect("cycle detected");
        assert!(matches!(cycle, AgentError::DependencyCycle(_)));
    }

    // 생성된 인라인 fallback은 metadata.fallback_of를 통해 원래 작업과 연결된다.
    #[test]
    fn inline_fallback_materialized_produces_main_to_fallback_edge() {
        let main = task(
            "main",
            "main-with-inline-fb",
            TaskState::Failed {
                error: "boom".to_string(),
            },
        );
        let mut fallback = task("fb", "inline-fallback", TaskState::Succeeded);
        fallback.metadata = json!({"fallback_of": "main"});
        let tasks = [main, fallback];
        let edges = collect_graph_edges(&tasks);
        assert_eq!(
            edges.len(),
            1,
            "expected exactly one edge, got: {edges:?}",
            edges = edges_debug(&edges)
        );
        assert_eq!(edges[0].from, "main");
        assert_eq!(edges[0].to, "fb");
        assert_eq!(edges[0].kind, "fallback");
    }

    // 작업이 실패하기 전에는 인라인 fallback이 아직 생성되지 않아 표시할 엣지도 없다.
    #[test]
    fn inline_fallback_not_yet_materialized_has_no_edge() {
        let main = task("main", "main-with-inline-fb", TaskState::Ready);
        let edges = collect_graph_edges(std::slice::from_ref(&main));
        assert!(
            edges.is_empty(),
            "expected no edges, got: {edges:?}",
            edges = edges_debug(&edges)
        );
    }

    // fallback_of는 참조 무결성 검사 대상이 아니므로 원래 작업이 먼저 삭제될 수 있다.
    #[test]
    fn inline_fallback_with_deleted_main_is_skipped_without_panic() {
        let mut fallback = task("fb", "inline-fallback", TaskState::Succeeded);
        fallback.metadata = json!({"fallback_of": "main-no-longer-exists"});
        let tasks = [fallback];
        let edges = collect_graph_edges(&tasks);
        assert!(
            edges.is_empty(),
            "expected no edges, got: {edges:?}",
            edges = edges_debug(&edges)
        );
    }

    #[test]
    fn depends_on_explicit_fallback_and_reduce_edges_are_unaffected() {
        let a = task("a", "dep-a", TaskState::Succeeded);
        let mut b = task("b", "dep-b", TaskState::Succeeded);
        b.depends_on = vec!["a".to_string()];

        let fb_target = task("fbtarget", "existing-fb-target", TaskState::Succeeded);
        let mut main = task(
            "main2",
            "main-with-existing-fb",
            TaskState::Failed {
                error: "boom".to_string(),
            },
        );
        main.on_failure = OnFailure::Fallback {
            task: Some("fbtarget".to_string()),
            inline: None,
        };

        let mut reducer = task("reduce", "reduce-ab", TaskState::Succeeded);
        reducer.command = TaskCommand::Reduce {
            inputs: vec!["a".to_string(), "b".to_string()],
            strategy: ReducerStrategy::All,
        };

        let tasks = vec![a, b, fb_target, main, reducer];
        let edges = collect_graph_edges(&tasks);

        let has = |from: &str, to: &str, kind: &str| {
            edges
                .iter()
                .any(|e| e.from == from && e.to == to && e.kind == kind)
        };
        assert!(
            has("a", "b", "depends_on"),
            "got: {edges:?}",
            edges = edges_debug(&edges)
        );
        assert!(
            has("main2", "fbtarget", "fallback"),
            "got: {edges:?}",
            edges = edges_debug(&edges)
        );
        assert!(
            has("a", "reduce", "reduce"),
            "got: {edges:?}",
            edges = edges_debug(&edges)
        );
        assert!(
            has("b", "reduce", "reduce"),
            "got: {edges:?}",
            edges = edges_debug(&edges)
        );
        assert_eq!(
            edges.len(),
            4,
            "got: {edges:?}",
            edges = edges_debug(&edges)
        );
    }

    fn edges_debug(edges: &[GraphEdge<'_>]) -> Vec<(String, String, &'static str)> {
        edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone(), e.kind))
            .collect()
    }
}

#[cfg(test)]
mod state_filter_tests {
    use super::*;

    fn task(id: &str, state: TaskState) -> Task {
        Task {
            id: id.to_string(),
            workspace_id: 1,
            name: id.to_string(),
            command: TaskCommand::Run {
                command: vec!["true".to_string()],
                workspace_id: 1,
                cwd: None,
            },
            depends_on: Vec::new(),
            state,
            created_at: 0,
            started_at: None,
            finished_at: None,
            result: None,
            on_failure: OnFailure::Abort,
            metadata: Value::Null,
            reserved_for_fallback: false,
        }
    }

    fn sample() -> Vec<Task> {
        vec![
            task("w", TaskState::Waiting),
            task("r", TaskState::Ready),
            task("run", TaskState::Running),
            task("ok", TaskState::Succeeded),
        ]
    }

    fn filtered(params: &Value) -> Vec<String> {
        let mut tasks = sample();
        retain_by_state(&mut tasks, state_names_param(params, "state").as_deref());
        tasks.into_iter().map(|t| t.id).collect()
    }

    #[test]
    fn single_state_string_filters_one_state() {
        assert_eq!(filtered(&json!({"state": "running"})), vec!["run"]);
    }

    #[test]
    fn comma_separated_states_match_any() {
        assert_eq!(
            filtered(&json!({"state": "waiting,ready,running"})),
            vec!["w", "r", "run"]
        );
        assert_eq!(
            filtered(&json!({"state": " waiting , running "})),
            vec!["w", "run"]
        );
    }

    #[test]
    fn array_states_match_any() {
        assert_eq!(
            filtered(&json!({"state": ["ready", "succeeded"]})),
            vec!["r", "ok"]
        );
    }

    #[test]
    fn no_match_yields_empty_list() {
        assert!(filtered(&json!({"state": "failed,cancelled"})).is_empty());
    }

    #[test]
    fn absent_or_empty_filter_keeps_everything() {
        for params in [
            json!({}),
            json!({"state": Value::Null}),
            json!({"state": ""}),
            json!({"state": ","}),
            json!({"state": []}),
        ] {
            assert_eq!(filtered(&params).len(), 4, "params: {params}");
        }
    }

    #[test]
    fn singular_and_plural_keys_are_interchangeable() {
        let p = json!({"states": "ready,running"});
        assert_eq!(
            state_names_param(&p, "state"),
            Some(vec!["ready".to_string(), "running".to_string()])
        );
        let p = json!({"state": "succeeded"});
        assert_eq!(
            state_names_param(&p, "states"),
            Some(vec!["succeeded".to_string()])
        );
    }
}

// 빈 상태 필터가 전체 삭제로 해석되지 않는지 실제 파싱과 plan_sweep을 함께 검사한다.
#[cfg(test)]
mod purge_state_filter_tests {
    use std::sync::atomic::AtomicU64;

    use tasty_agent::TaskStore;
    use tasty_agent::task::TaskSweepPlan;
    use tasty_memory::MemoryStorage;

    use super::*;

    fn plan(params: &Value) -> TaskSweepPlan {
        let mut mem = tasty_memory::testing::InMemoryStorage::new();
        let seq = AtomicU64::new(0);
        let mem_dyn: &mut dyn MemoryStorage = &mut mem;
        let mut store = TaskStore::new(mem_dyn, "_host", &seq);
        for name in ["a", "b"] {
            store
                .create(TaskCreateOpts {
                    workspace_id: 1,
                    name: name.to_string(),
                    command: TaskCommand::Run {
                        command: vec!["true".to_string()],
                        workspace_id: 1,
                        cwd: None,
                    },
                    depends_on: Vec::new(),
                    on_failure: OnFailure::Abort,
                    metadata: Value::Null,
                    now_ms: 1000,
                })
                .expect("create");
        }
        let filter = purge_filter_from_params(params, 100_000).expect("픽스처 params 는 정상");
        store.plan_sweep(1, &filter).expect("plan_sweep")
    }

    #[test]
    fn empty_states_array_purges_nothing() {
        let p = plan(&json!({"states": [], "older_than_ms": 1000}));
        assert!(
            p.deleted.is_empty() && p.retained.is_empty(),
            "빈 states 는 매칭 0 건이어야 한다: {p:?}"
        );
    }

    #[test]
    fn empty_state_string_purges_nothing() {
        for params in [
            json!({"state": "", "older_than_ms": 1000}),
            json!({"state": ",", "older_than_ms": 1000}),
            json!({"states": [""], "older_than_ms": 1000}),
        ] {
            let p = plan(&params);
            assert!(
                p.deleted.is_empty() && p.retained.is_empty(),
                "params: {params} → {p:?}"
            );
        }
    }

    #[test]
    fn absent_states_key_still_means_no_state_filter() {
        let p = plan(&json!({"older_than_ms": 1000}));
        assert_eq!(p.deleted.len(), 2, "{p:?}");
    }

    #[test]
    fn named_states_still_purge() {
        let p = plan(&json!({"states": ["ready"], "older_than_ms": 1000}));
        assert_eq!(p.deleted.len(), 2, "{p:?}");
        let p = plan(&json!({"states": ["failed"], "older_than_ms": 1000}));
        assert!(p.deleted.is_empty() && p.retained.is_empty(), "{p:?}");
    }

    #[test]
    fn keep_empty_variant_preserves_key_presence() {
        assert_eq!(state_names_param_keep_empty(&json!({}), "states"), None);
        assert_eq!(
            state_names_param_keep_empty(&json!({"states": Value::Null}), "states"),
            None
        );
        assert_eq!(
            state_names_param_keep_empty(&json!({"states": []}), "states"),
            Some(Vec::new())
        );
        assert_eq!(
            state_names_param_keep_empty(&json!({"state": ""}), "states"),
            Some(Vec::new())
        );
        assert_eq!(
            state_names_param_keep_empty(&json!({"states": ["ready"]}), "states"),
            Some(vec!["ready".to_string()])
        );
        assert_eq!(state_names_param(&json!({"states": []}), "state"), None);
    }
}
