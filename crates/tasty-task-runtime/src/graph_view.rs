//! DOT·JSON·DAG 화면이 같은 작업 간 연결을 사용하도록 함께 수집한다.

use tasty_agent::{OnFailure, Task, TaskCommand, TaskId};

pub struct GraphEdge<'a> {
    pub from: &'a TaskId,
    pub to: &'a TaskId,
    pub kind: &'static str,
    /// `transition` 엣지의 선택 상태. 다른 종류는 `None`.
    pub selection: Option<&'static str>,
}

/// DOT·DAG 화면이 그릴 간선. binding 이 이미 순서를 뜻하므로 같은 쌍의 depends_on 은 그리지 않는다.
pub fn drawn_edges(edges: Vec<GraphEdge<'_>>) -> Vec<GraphEdge<'_>> {
    let bound: Vec<(&TaskId, &TaskId)> = edges
        .iter()
        .filter(|e| e.kind == "binding")
        .map(|e| (e.from, e.to))
        .collect();
    edges
        .into_iter()
        .filter(|e| e.kind != "depends_on" || !bound.contains(&(e.from, e.to)))
        .collect()
}

/// 직접 참조는 그대로 연결한다. inline fallback은 실패 뒤 만든 작업의 fallback_of를 역조회한다.
/// 원래 작업이 삭제될 수 있어 fallback_of 대상이 없으면 그 연결은 생략한다.
/// 같은 쌍에 depends_on 과 binding 이 함께 있어도 둘 다 낸다. JSON 은 선언을 그대로 전하는 API 이고,
/// 한 줄로 줄이는 것은 [`drawn_edges`] 를 쓰는 그리기 쪽의 일이다.
pub fn collect_graph_edges(tasks: &[Task]) -> Vec<GraphEdge<'_>> {
    let mut edges = Vec::new();
    for t in tasks {
        let bound = tasty_agent::task::binding_task_ids(t);
        for dep in &t.depends_on {
            edges.push(GraphEdge {
                from: dep,
                to: &t.id,
                kind: "depends_on",
                selection: None,
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
                selection: None,
            });
        }
        if let TaskCommand::Reduce { inputs, .. } = &t.command {
            for input in inputs {
                edges.push(GraphEdge {
                    from: input,
                    to: &t.id,
                    kind: "reduce",
                    selection: None,
                });
            }
        }
        // v2 입력 binding 은 값을 전달하는 데이터 엣지다. 같은 source 를 여러 필드가 읽어도 한 번만 그린다.
        let mut sources: Vec<&TaskId> = bound;
        sources.sort();
        sources.dedup();
        for source in sources {
            edges.push(GraphEdge {
                from: source,
                to: &t.id,
                kind: "binding",
                selection: None,
            });
        }
        // v2 전이는 성공한 출력으로 대상을 고르는 제어 엣지다.
        for target in tasty_agent::task::route::transition_targets(t) {
            edges.push(GraphEdge {
                from: &t.id,
                to: target,
                kind: "transition",
                selection: Some(transition_selection(t, target)),
            });
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
            selection: None,
        });
    }
    edges
}

/// 전이 엣지의 선택 상태. 출처가 끝나기 전은 `pending`, 성공해 대상을 골랐으면 `selected`,
/// 고르지 않았거나 출처 자신이 선택되지 않았으면 `not_selected`, 그 밖에 성공 결과가 없으면
/// `unavailable` 이다.
pub fn transition_selection(source: &Task, target: &TaskId) -> &'static str {
    use tasty_agent::task::route::{self, EdgeStatus};
    match route::control_status(source, target) {
        EdgeStatus::Pending => "pending",
        EdgeStatus::Available => "selected",
        EdgeStatus::NotSelected => "not_selected",
        EdgeStatus::Unavailable { .. } => "unavailable",
    }
}

/// 노드 아이콘과 task_graph JSON이 공유하는 종류 식별자.
pub fn task_command_kind(command: &TaskCommand) -> &'static str {
    match command {
        TaskCommand::Run { .. } => "run",
        TaskCommand::Custom { .. } => "custom",
        TaskCommand::Reduce { .. } => "reduce",
        TaskCommand::WaitBarrier { .. } => "wait_barrier",
        TaskCommand::Agent { .. } => "agent",
    }
}

pub fn on_failure_kind(on_failure: &OnFailure) -> &'static str {
    match on_failure {
        OnFailure::Abort => "abort",
        OnFailure::ContinueDownstream => "continue_downstream",
        OnFailure::Fallback { .. } => "fallback",
    }
}
