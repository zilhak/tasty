//! task_graph·dag_get 응답의 그래프 렌더(json 노드·엣지와 dot)를 만든다.

use serde_json::{Value, json};
use tasty_agent::{AgentError, Task, TaskGraph, TaskState};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_task_runtime::graph_view::{
    collect_graph_edges, drawn_edges, on_failure_kind, task_command_kind,
};

use super::escape_dot;

/// task_graph와 dag_get의 공통 dot 렌더. 엣지 수집은 json과 collect_graph_edges를 공유한다.
/// depends_on/Reduce.inputs는 의존성 엣지다. fallback도 표시하지만 사이클 검사 대상은 아니다.
pub(super) fn render_graph_dot(tasks: &[Task]) -> String {
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
            TaskState::Unknown { .. } => "orange",
        };
        // 갈래에서 고르지 않아 건너뛴 task 는 실패 전파로 건너뛴 task 와 구분해 라벨과 테두리를 바꾼다.
        let not_selected = matches!(t.skip, Some(tasty_agent::SkipReason::BranchNotSelected));
        let (status, style) = if not_selected {
            ("not selected", "\"filled,dashed\"")
        } else {
            (t.state.name(), "filled")
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\\n{}\", style={}, fillcolor={}];\n",
            t.id,
            escape_dot(&t.name),
            status,
            style,
            color
        ));
    }
    for edge in drawn_edges(collect_graph_edges(tasks)) {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\"{};\n",
            edge.from,
            edge.to,
            dot_edge_attrs(edge.kind, edge.selection)
        ));
    }
    out.push_str("}\n");
    out
}

/// DOT 간선 속성. 관계는 선 모양·색과 라벨로 구분한다. Graphviz 에는 임의 파선 배열이 없어
/// 파선 관계(fallback·binding·transition)는 색과 라벨로 가른다. 전이는 선택 상태를 라벨로 싣고,
/// 선택된 간선은 굵게, 선택되지 않았거나 쓸 수 없는 간선은 반투명으로 그린다.
pub(super) fn dot_edge_attrs(kind: &str, selection: Option<&str>) -> String {
    match (kind, selection) {
        ("fallback", _) => " [style=dashed, color=orangered, label=\"fallback\"]".to_string(),
        ("reduce", _) => " [style=dotted, color=blue, label=\"reduce\"]".to_string(),
        ("binding", _) => " [style=dashed, color=darkcyan, label=\"binding\"]".to_string(),
        ("transition", Some(sel)) => {
            let attrs = match sel {
                "selected" => "color=slateblue, penwidth=2",
                "not_selected" | "unavailable" => "color=\"#6a5acd66\", fontcolor=gray",
                _ => "color=slateblue",
            };
            format!(" [style=dashed, {attrs}, label=\"{sel}\"]")
        }
        _ => String::new(),
    }
}

pub(super) fn render_graph_nodes(tasks: &[Task]) -> Vec<Value> {
    tasks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "state": t.state.name(),
                "command_kind": task_command_kind(&t.command),
                "on_failure_kind": on_failure_kind(&t.on_failure),
                "skip": t.skip,
            })
        })
        .collect()
}

/// depends_on, Reduce.inputs, Fallback.task와 인라인 fallback의 역참조를 구분해 표시한다.
/// fallback 순환은 사이클 검사 대상이 아니므로 표시만 하며 생성 시 거절하지 않는다.
pub(super) fn render_graph_edges(tasks: &[Task]) -> Vec<Value> {
    collect_graph_edges(tasks)
        .into_iter()
        .map(|edge| match edge.selection {
            Some(selection) => json!({"from": edge.from, "to": edge.to, "kind": edge.kind,
                                      "selection": selection}),
            None => json!({"from": edge.from, "to": edge.to, "kind": edge.kind}),
        })
        .collect()
}

pub(super) fn graph_response(
    id: Value,
    format: &str,
    tasks: &[Task],
    runner: Value,
) -> JsonRpcResponse {
    // 사이클이 있어도 그래프는 반환한다.
    let cycle = TaskGraph::build(tasks).detect_cycles().err();
    match format {
        "dot" => JsonRpcResponse::success(
            id,
            json!({
                "format": "dot",
                "dot": render_graph_dot(tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
        _ => {
            let nodes: Vec<Value> = render_graph_nodes(tasks);
            let edges: Vec<Value> = render_graph_edges(tasks);
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

/// 목록 조회의 전송량을 줄이기 위해 요청한 경우에만 task_ids를 포함한다.
pub(super) fn dag_summary_json(dag: &tasty_agent::DagSummary, include_tasks: bool) -> Value {
    let mut v = serde_json::to_value(dag).unwrap_or(Value::Null);
    if !include_tasks && let Some(obj) = v.as_object_mut() {
        obj.remove("task_ids");
    }
    v
}

/// 명시적으로 묶은 DAG는 다른 그룹의 작업에 의존할 수 있다.
/// 이 경우 UnknownDependency를 사이클로 보고하지 않는다. DagSummary::has_cycle과 같은 기준이다.
/// workspace 전체 조회에서는 실제 끊어진 참조이므로 이 예외를 적용하지 않는다.
pub(super) fn subset_cycle(tasks: &[Task]) -> Option<AgentError> {
    match TaskGraph::build(tasks).detect_cycles() {
        Err(e @ AgentError::DependencyCycle(_)) => Some(e),
        _ => None,
    }
}

pub(super) fn dag_response(
    id: Value,
    format: &str,
    dag: &tasty_agent::DagSummary,
    tasks: &[Task],
    runner: Value,
) -> JsonRpcResponse {
    let cycle = subset_cycle(tasks);
    let summary = dag_summary_json(dag, true);

    match format {
        "dot" => JsonRpcResponse::success(
            id,
            json!({
                "format": "dot",
                "dag": summary,
                "dot": render_graph_dot(tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
        _ => JsonRpcResponse::success(
            id,
            json!({
                "format": "json",
                "dag": summary,
                "nodes": render_graph_nodes(tasks),
                "edges": render_graph_edges(tasks),
                "cycle": cycle.as_ref().map(|e| e.to_string()),
                "runner": runner,
            }),
        ),
    }
}
