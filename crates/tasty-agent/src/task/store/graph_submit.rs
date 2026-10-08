//! v2 task 그래프의 전체 검증과 활성화.
//!
//! 앞뒤 노드를 서로 참조하는 정의는 task 를 하나씩 만들면 일부만 실행될 수 있다. 그래서
//! 그래프 전체를 먼저 검증하고([`TaskStore::plan_graph`]), 통과한 정의만 저장한다. 저장은
//! 두 단계다. task 를 활성화 전 상태로 쓰고([`TaskStore::stage_graph`]), 활성화 레코드 하나를
//! 쓴 뒤 readiness 를 평가한다([`TaskStore::activate_graph`]). 활성화 레코드가 없는 그래프의
//! task 는 readiness 평가에서 항상 대기라 실행되지 않는다. 검증에 실패하면 아무것도 쓰지
//! 않고, 쓰는 도중 실패하면 이미 쓴 task 를 지운다.

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;
use serde_json::{Value, json};
use tasty_memory::{ListOpts, MemoryValue, PutOpts, Scope};
use tasty_utils::id::WorkspaceId;

use super::super::binding::{InputBinding, InputMapping};
use super::super::contract::{self, FailureStage, MergeConflict, TaskContract, TaskFailure};
use super::super::postprocess::PostprocessSpec;
use super::super::record_limit;
use super::super::route::{self, Transitions};
use super::super::types::TypeSchema;
use super::super::{
    OnFailure, TASK_GRAPH_KEY_PREFIX, TASK_GRAPH_RECORD_FORMAT, Task, TaskCommand, TaskGraph,
    TaskId, TaskState, typed_task_key,
};
use super::{TaskStore, graph_key};
use crate::{AgentError, Result};

/// 그래프 하나에 담을 수 있는 task 수의 상한. 제출은 memory 잠금을 쥔 채 활성화하고
/// 활성화 비용이 task 수의 제곱으로 늘어난다. 측정값과 근거는 ADR-0068.
pub const MAX_GRAPH_TASKS: usize = 1000;

#[cfg(test)]
thread_local! {
    /// 시험 전용: 활성화 3단계(readiness 반영)의 첫 저장을 실패시킨다.
    pub(crate) static FAIL_ACTIVATION_PUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 한 번에 제출하는 v2 task 그래프. 모든 task 는 같은 `types` 를 쓴다.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGraphSpec {
    pub contract_version: u32,
    #[serde(default)]
    pub types: BTreeMap<String, TypeSchema>,
    pub tasks: Vec<GraphTaskSpec>,
    /// 재시작 뒤에도 이어서 실행돼야 하는가. 저장소가 영속이 아닐 때 활성화할지를 정한다.
    #[serde(default)]
    pub durability: GraphDurability,
}

/// 그래프가 요구하는 영속성.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDurability {
    /// 재시작 복구를 요구한다. 영속이 아닌 저장소에서는 활성화하지 않는다.
    #[default]
    Required,
    /// 영속이 아닌 저장소에서도 실행한다. 재시작하면 그래프와 결과가 사라질 수 있다.
    BestEffort,
}

/// 그래프 안의 task 하나. `id` 는 호출자가 정하며 그대로 task id 가 된다. 같은 그래프의
/// 다른 task 와 workspace 의 기존 task 를 id 로 참조한다.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphTaskSpec {
    pub id: TaskId,
    #[serde(default)]
    pub name: Option<String>,
    pub command: TaskCommand,
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub input_schema: Option<TypeSchema>,
    #[serde(default)]
    pub output_schema: Option<TypeSchema>,
    #[serde(default)]
    pub bindings: BTreeMap<String, InputBinding>,
    #[serde(default)]
    pub input_mapping: Option<InputMapping>,
    #[serde(default)]
    pub allowed_exit_codes: Option<Vec<i32>>,
    #[serde(default)]
    pub merge_conflict: Option<MergeConflict>,
    #[serde(default)]
    pub postprocess: Option<PostprocessSpec>,
    #[serde(default)]
    pub transitions: Option<Transitions>,
}

/// 검증을 통과한 그래프. task 는 아직 저장하지 않았고 상태는 Waiting 이다.
#[derive(Debug, Clone)]
pub struct GraphPlan {
    pub workspace_id: WorkspaceId,
    pub graph_id: String,
    pub tasks: Vec<Task>,
    pub durability: GraphDurability,
}

fn graph_error(message: impl Into<String>, location: impl Into<String>) -> AgentError {
    AgentError::TypeContract(Box::new(TaskFailure {
        location: Some(location.into()),
        ..TaskFailure::new(FailureStage::Input, message)
    }))
}

impl TaskStore<'_> {
    /// 그래프 전체를 검증하고 저장할 task 를 만든다. 저장소는 바꾸지 않는다.
    pub fn plan_graph(
        &self,
        workspace_id: WorkspaceId,
        spec: TaskGraphSpec,
        now_ms: u64,
    ) -> Result<GraphPlan> {
        if spec.contract_version != contract::TASK_CONTRACT_V2 {
            return Err(graph_error(
                format!(
                    "unsupported contract_version {} (supported: {})",
                    spec.contract_version,
                    contract::TASK_CONTRACT_V2
                ),
                "/contract_version",
            ));
        }
        if spec.tasks.is_empty() {
            return Err(graph_error(
                "a task graph needs at least one task",
                "/tasks",
            ));
        }
        if spec.tasks.len() > MAX_GRAPH_TASKS {
            return Err(graph_error(
                format!(
                    "a task graph holds at most {MAX_GRAPH_TASKS} tasks, got {}; split it into smaller graphs",
                    spec.tasks.len()
                ),
                "/tasks",
            ));
        }
        let existing = self.list(workspace_id)?;
        // task ID 와 같은 이유로, 살아 있는 그래프가 이미 쓰는 ID 는 건너뛴다.
        let graph_id = loop {
            let id = format!(
                "g-{now_ms}-{:06}",
                self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            if !existing
                .iter()
                .any(|t| t.graph_id.as_deref() == Some(id.as_str()))
            {
                break id;
            }
        };
        let existing_ids: HashSet<&TaskId> = existing.iter().map(|t| &t.id).collect();

        let durability = spec.durability;
        let mut planned: Vec<Task> = Vec::with_capacity(spec.tasks.len());
        for (i, t) in spec.tasks.into_iter().enumerate() {
            let at = format!("/tasks/{i}");
            if let Err(e) = typed_task_key(&t.id)
                .and_then(|_| super::super::report::check_report_key_room(&t.id))
            {
                return Err(graph_error(e.to_string(), format!("{at}/id")));
            }
            if existing_ids.contains(&t.id) {
                return Err(graph_error(
                    format!("task id {} already exists in this workspace", t.id),
                    format!("{at}/id"),
                ));
            }
            if planned.iter().any(|p| p.id == t.id) {
                return Err(graph_error(
                    format!("task id {} appears more than once in the graph", t.id),
                    format!("{at}/id"),
                ));
            }
            let mut metadata = match t.metadata {
                Value::Null => json!({}),
                Value::Object(m) => Value::Object(m),
                _ => {
                    return Err(graph_error(
                        "metadata must be an object",
                        format!("{at}/metadata"),
                    ));
                }
            };
            // 제출한 그래프를 한 표시 그룹으로 묶는다. 호출자가 정한 그룹은 그대로 둔다.
            if let Some(m) = metadata.as_object_mut() {
                m.entry("dag")
                    .or_insert_with(|| Value::String(graph_id.clone()));
            }
            planned.push(Task {
                name: t.name.unwrap_or_else(|| t.id.clone()),
                id: t.id,
                workspace_id,
                command: t.command,
                depends_on: t.depends_on,
                state: TaskState::Waiting,
                created_at: now_ms,
                started_at: None,
                finished_at: None,
                result: None,
                on_failure: t.on_failure,
                metadata,
                reserved_for_fallback: false,
                contract: Some(TaskContract {
                    contract_version: spec.contract_version,
                    types: spec.types.clone(),
                    input_schema: t.input_schema,
                    output_schema: t.output_schema,
                    allowed_exit_codes: t.allowed_exit_codes,
                    merge_conflict: t.merge_conflict,
                    bindings: t.bindings,
                    input_mapping: t.input_mapping,
                    postprocess: t.postprocess,
                    transitions: t.transitions,
                }),
                typed_result: None,
                graph_id: Some(graph_id.clone()),
                input_snapshot: None,
                accepted: None,
                report_token: None,
                attempt: None,
                route: None,
                skip: None,
            });
        }

        let lookup = |id: &TaskId| {
            planned
                .iter()
                .find(|t| &t.id == id)
                .or_else(|| existing.iter().find(|t| &t.id == id))
        };
        for (i, t) in planned.iter().enumerate() {
            let at = format!("/tasks/{i}");
            for (j, dep) in t.depends_on.iter().enumerate() {
                if lookup(dep).is_none() {
                    return Err(graph_error(
                        format!("task {}: depends_on task not found: {dep}", t.id),
                        format!("{at}/depends_on/{j}"),
                    ));
                }
            }
            if let TaskCommand::Reduce { inputs, .. } = &t.command {
                for (j, input) in inputs.iter().enumerate() {
                    if lookup(input).is_none() {
                        return Err(graph_error(
                            format!("task {}: reduce input task not found: {input}", t.id),
                            format!("{at}/command/inputs/{j}"),
                        ));
                    }
                }
            }
            if let OnFailure::Fallback { task: Some(fb), .. } = &t.on_failure
                && lookup(fb).is_none()
            {
                return Err(graph_error(
                    format!("task {}: fallback task not found: {fb}", t.id),
                    format!("{at}/on_failure/task"),
                ));
            }
            if let Err(message) = record_limit::check(t, "the definition", self.record_limit()) {
                return Err(graph_error(message, at));
            }
            contract::check_task(t, &at, lookup)
                .map_err(|f| AgentError::TypeContract(Box::new(f)))?;
            route::check_transitions(t, &at, &planned, &lookup)
                .map_err(|f| AgentError::TypeContract(Box::new(f)))?;
        }
        let everything: Vec<&Task> = existing.iter().chain(planned.iter()).collect();
        route::check_route_inputs(&planned, &everything)
            .map_err(|f| AgentError::TypeContract(Box::new(f)))?;

        let mut all = existing.clone();
        all.extend(planned.iter().cloned());
        if let Err(AgentError::DependencyCycle(cycle)) = TaskGraph::build(&all).detect_cycles() {
            return Err(graph_error(
                format!("dependency cycle: {}", cycle.join(" -> ")),
                "/tasks",
            ));
        }
        Ok(GraphPlan {
            workspace_id,
            graph_id,
            tasks: planned,
            durability,
        })
    }

    /// 검증한 그래프의 task 를 활성화 전 상태로 저장한다. 도중에 실패하면 이미 쓴 task 를 지운다.
    pub fn stage_graph(&mut self, plan: &GraphPlan) -> Result<()> {
        for (i, t) in plan.tasks.iter().enumerate() {
            if let Err(e) = self.put(t) {
                self.rollback_staged(plan.workspace_id, &plan.tasks[..i]);
                return Err(e);
            }
        }
        Ok(())
    }

    fn rollback_staged(&mut self, workspace_id: WorkspaceId, written: &[Task]) {
        for t in written {
            if let Err(e) = self.delete(workspace_id, &t.id) {
                // 남은 task 는 활성화 레코드가 없어 실행되지 않는다. 정리 대상으로 알린다.
                tracing::warn!(
                    task_id = %t.id,
                    "rolling back a staged graph task failed; it stays inactive: {e}"
                );
            }
        }
    }

    /// 활성화 레코드를 쓰고 그래프 task 의 readiness 를 평가한다. 반환값은 활성화 뒤 그래프의
    /// task 전체다. 레코드를 쓰지 못하면 저장한 task 를 지운다.
    pub fn activate_graph(&mut self, plan: &GraphPlan, now_ms: u64) -> Result<Vec<Task>> {
        let ws = plan.workspace_id;
        let record = json!({
            "record_format": TASK_GRAPH_RECORD_FORMAT,
            "graph_id": plan.graph_id,
            "task_ids": plan.tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
            "activated_at": now_ms,
            "durability": plan.durability,
        });
        let put = graph_key(&plan.graph_id).and_then(|key| {
            self.mem
                .put(
                    &self.owner,
                    &Scope::Workspace(ws),
                    &key,
                    &MemoryValue::Json(record),
                    &PutOpts::default(),
                )
                .map_err(AgentError::from)
        });
        if let Err(e) = put {
            self.rollback_staged(ws, &plan.tasks);
            return Err(e);
        }
        // 레코드를 쓴 뒤에는 그래프가 활성이다. 이후 실패는 롤백하지 않고 그 사실을 오류에 싣는다.
        self.apply_graph_readiness(plan, now_ms)
            .map_err(|e| AgentError::GraphPartiallyActivated {
                graph_id: plan.graph_id.clone(),
                source: Box::new(e),
            })
    }

    /// 활성화 3단계. 그래프 task 의 readiness 를 평가해 저장하고 하류로 전파한다.
    fn apply_graph_readiness(&mut self, plan: &GraphPlan, now_ms: u64) -> Result<Vec<Task>> {
        let ws = plan.workspace_id;
        let mut all = self.list(ws)?;
        let ids: Vec<TaskId> = plan.tasks.iter().map(|t| t.id.clone()).collect();
        let settled: Vec<TaskId> = self
            .settle_waiting(ws, &mut all, &ids, now_ms)?
            .into_iter()
            .filter(|t| t.state.is_terminal())
            .map(|t| t.id)
            .collect();
        for id in settled {
            self.cascade_downstream(ws, &id, now_ms)?;
        }
        let ids: HashSet<&TaskId> = plan.tasks.iter().map(|t| &t.id).collect();
        Ok(self
            .list(ws)?
            .into_iter()
            .filter(|t| ids.contains(&t.id))
            .collect())
    }

    /// 검증, 저장, 활성화를 한 번에 한다.
    pub fn submit_graph(
        &mut self,
        workspace_id: WorkspaceId,
        spec: TaskGraphSpec,
        now_ms: u64,
    ) -> Result<(String, Vec<Task>)> {
        let plan = self.plan_graph(workspace_id, spec, now_ms)?;
        self.stage_graph(&plan)?;
        let tasks = self.activate_graph(&plan, now_ms)?;
        Ok((plan.graph_id, tasks))
    }

    /// 남은 task 가 없는 그래프의 활성화 레코드를 지운다.
    pub(super) fn prune_graph_records(&mut self, workspace_id: WorkspaceId) -> Result<()> {
        let scope = Scope::Workspace(workspace_id);
        let records = self.mem.list(
            &scope,
            &ListOpts {
                prefix: Some(TASK_GRAPH_KEY_PREFIX.to_string()),
                ..Default::default()
            },
        )?;
        if records.is_empty() {
            return Ok(());
        }
        let live: HashSet<String> = self
            .list(workspace_id)?
            .into_iter()
            .filter_map(|t| t.graph_id)
            .collect();
        for r in records {
            let gid = r.key.trim_start_matches(TASK_GRAPH_KEY_PREFIX);
            if !live.contains(gid) {
                self.mem.delete(&self.owner, &scope, &r.key, None)?;
            }
        }
        Ok(())
    }
}
