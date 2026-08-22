//! Task 그래프의 **엣지 수집** — 렌더 형식과 무관한 단일 지점.
//!
//! 세 소비자가 같은 답을 봐야 한다: `agent.task_graph` 의 dot 브랜치, 같은 핸들러의
//! json 브랜치(= `agent.dag_get` 공유), 그리고 DAG surface 의 캔버스
//! (`src/adapters/ui/surface/dag_graph/`). 각자 손으로 매칭을 적으면 "무엇이 엣지
//! 인가" 가 갈라진다 — 실제로 예전에 dot/json 이 각자 `Fallback{task}` 매칭을
//! 따로 쓰다 둘 다 `Fallback{inline}` 을 아예 안 그렸다.

use tasty_agent::{OnFailure, Task, TaskCommand, TaskId};

/// 방향 있는 엣지 하나. `kind` 는 `"depends_on"`/`"fallback"`/`"reduce"` 이며 dot
/// 렌더에서는 엣지 `label` 로도 쓰인다.
pub(crate) struct GraphEdge<'a> {
    pub(crate) from: &'a TaskId,
    pub(crate) to: &'a TaskId,
    pub(crate) kind: &'static str,
}

/// 렌더가 그려야 할 엣지 전부를 모은다.
///
/// `depends_on` / `Fallback{task}` / `Reduce.inputs` 는 `referenced_task_ids` 와 정확히
/// 같은 3 종 참조다(그 crate 의 생성/삭제 무결성 검사가 이미 권위로 취급하는 것들)
/// — 그중 dangling id 는 정상 경로에서 나올 수 없다.
///
/// inline fallback 엣지만 성격이 다르다: main 의 `on_failure` 에는 대상 id 자체가
/// 없고(`Fallback { task: None, inline: Some(_) }` — 생성 시점에 대상이 아직 없다,
/// 실패 시에 만들어진다) 관계가 기록되는 유일한 자리는 *fallback* task 쪽의
/// `metadata.fallback_of == main.id` 다. `referenced_task_ids` 가 의도적으로 제외한
/// 역방향 조회라 무결성 보증이 없다 — main 이 그 뒤 삭제됐으면 `fallback_of` 는
/// dangling 일 수 있다. 여기서는 그게 예외가 아니라 예상 상태이므로, 에러 대신
/// 그 엣지를 조용히 건너뛴다(아직 materialize 되지 않은 inline fallback 이 그릴
/// 엣지가 없는 것과 같은 취급).
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

/// `TaskCommand` 의 안정 식별자 — 노드 카드의 종류 아이콘과 `agent.task_graph` 의
/// json `command_kind` 가 같은 어휘를 쓴다.
pub(crate) fn task_command_kind(command: &TaskCommand) -> &'static str {
    match command {
        TaskCommand::Run { .. } => "run",
        TaskCommand::Custom { .. } => "custom",
        TaskCommand::Reduce { .. } => "reduce",
        TaskCommand::WaitBarrier { .. } => "wait_barrier",
    }
}

/// `OnFailure` 의 안정 식별자.
pub(crate) fn on_failure_kind(on_failure: &OnFailure) -> &'static str {
    match on_failure {
        OnFailure::Abort => "abort",
        OnFailure::ContinueDownstream => "continue_downstream",
        OnFailure::Fallback { .. } => "fallback",
    }
}
