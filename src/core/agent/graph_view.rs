//! DOT·JSON·DAG 화면이 같은 작업 간 연결을 사용하도록 함께 수집한다.

use tasty_agent::{OnFailure, Task, TaskCommand, TaskId};

pub(crate) struct GraphEdge<'a> {
    pub(crate) from: &'a TaskId,
    pub(crate) to: &'a TaskId,
    pub(crate) kind: &'static str,
}

/// 직접 참조는 그대로 연결한다. inline fallback은 실패 뒤 만든 작업의 fallback_of를 역조회한다.
/// 원래 작업이 삭제될 수 있어 fallback_of 대상이 없으면 그 연결은 생략한다.
pub(crate) fn collect_graph_edges(tasks: &[Task]) -> Vec<GraphEdge<'_>> {
    let mut edges = Vec::new();
    for t in tasks {
        for dep in &t.depends_on {
            edges.push(GraphEdge {
                from: dep,
                to: &t.id,
                kind: "depends_on",
            });
        }
        if let OnFailure::Fallback {
            task: Some(fb_id), ..
        } = &t.on_failure
        {
            edges.push(GraphEdge {
                from: &t.id,
                to: fb_id,
                kind: "fallback",
            });
        }
        if let TaskCommand::Reduce { inputs, .. } = &t.command {
            for input in inputs {
                edges.push(GraphEdge {
                    from: input,
                    to: &t.id,
                    kind: "reduce",
                });
            }
        }
    }
    for fb in tasks {
        let Some(main_id) = fb.metadata.get("fallback_of").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(main) = tasks.iter().find(|t| t.id == main_id) else {
            continue;
        };
        edges.push(GraphEdge {
            from: &main.id,
            to: &fb.id,
            kind: "fallback",
        });
    }
    edges
}

/// 노드 아이콘과 task_graph JSON이 공유하는 종류 식별자.
pub(crate) fn task_command_kind(command: &TaskCommand) -> &'static str {
    match command {
        TaskCommand::Run { .. } => "run",
        TaskCommand::Custom { .. } => "custom",
        TaskCommand::Reduce { .. } => "reduce",
        TaskCommand::WaitBarrier { .. } => "wait_barrier",
    }
}

pub(crate) fn on_failure_kind(on_failure: &OnFailure) -> &'static str {
    match on_failure {
        OnFailure::Abort => "abort",
        OnFailure::ContinueDownstream => "continue_downstream",
        OnFailure::Fallback { .. } => "fallback",
    }
}
