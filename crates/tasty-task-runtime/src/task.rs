//! TaskService의 작업 API. 원본은 memory의 TaskStore이며 engine별 순번·허브는 TaskScope로 받는다.

use std::collections::HashSet;

use tasty_agent::task::{
    Completion, CompletionOutcome, CompletionReceipt, GraphDurability, TaskCreateOpts,
    TaskDeleteOpts, TaskDeleteReport, TaskGraphSpec, TaskPurgeFilter, TaskSweepPlan,
};
use tasty_agent::{
    AgentError, DagSummary, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore,
    group_tasks_into_dags,
};
use tasty_memory::HOST_OWNER;

use crate::runner_host::evict_task_side_keys;
use crate::{TaskScope, TaskService};

impl TaskService {
    /// fallback 예약 작업은 참조할 본 작업이 등록되기 전에 Ready가 되지 않게 만든다.
    pub fn task_create(
        &self,
        scope: &TaskScope,
        opts: TaskCreateOpts,
        reserved_for_fallback: bool,
    ) -> Result<Task, AgentError> {
        crate::runner_host::check_holding_ttls(&opts.name, &opts.metadata, &opts.on_failure)
            .map_err(AgentError::InvalidArgument)?;
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            reject_v1_reads_of_typed(&store, opts.workspace_id, &opts.command, &opts.on_failure)?;
            if reserved_for_fallback {
                store.create_reserved_for_fallback(opts)
            } else {
                store.create(opts)
            }
        })
    }

    /// v2 task 그래프를 전체 검증하고, `dry_run` 이 아니면 저장·활성화한다. 검증에 실패하면
    /// 아무것도 저장하지 않는다. 활성화 전에는 그래프의 어떤 task 도 실행 대상이 아니다.
    pub fn task_graph_submit(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        spec: TaskGraphSpec,
        dry_run: bool,
        now_ms: u64,
    ) -> Result<GraphSubmitOutcome, AgentError> {
        // 검증만 할 때도 같은 판정을 해 제출 결과를 미리 알 수 있게 한다.
        if let Some(cause) = self.store_fallback()
            && spec.durability == GraphDurability::Required
        {
            return Err(AgentError::StoreNotDurable {
                cause: cause.to_string(),
            });
        }
        let seq = scope.agent_seq().clone();
        let outcome = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let plan = store.plan_graph(workspace_id, spec, now_ms)?;
            for (i, t) in plan.tasks.iter().enumerate() {
                reject_output_placeholders(i, t)?;
                crate::runner_host::check_holding_ttls(&t.id, &t.metadata, &t.on_failure)
                    .map_err(AgentError::InvalidArgument)?;
            }
            if dry_run {
                return Ok(GraphSubmitOutcome {
                    graph_id: plan.graph_id,
                    activated: false,
                    tasks: plan.tasks,
                });
            }
            store.stage_graph(&plan)?;
            let tasks = store.activate_graph(&plan, now_ms)?;
            Ok(GraphSubmitOutcome {
                graph_id: plan.graph_id,
                activated: true,
                tasks,
            })
        });
        if let Ok(o) = &outcome {
            for t in &o.tasks {
                self.fire_waker_if_terminal(scope, workspace_id, t);
            }
        }
        outcome
    }

    pub fn task_list(&self, scope: &TaskScope, workspace_id: u32) -> Result<Vec<Task>, AgentError> {
        task_list_from_state(self.memory(), scope, workspace_id)
    }

    /// 받은 workspace만 순회한다. 화면의 DAG 목록과 같은 구현을 쓴다.
    pub fn dag_list(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
    ) -> Result<Vec<DagSummary>, AgentError> {
        dag_list_from_state(self.memory(), scope, workspace_ids)
    }

    /// 받은 순서의 첫 일치를 반환한다. dag_scan_workspaces는 ID 오름차순으로 넘긴다.
    /// 사용자가 정한 DAG 키가 여러 workspace에 같을 수 있어 구별하려면 workspace_id도 지정한다.
    pub fn dag_get(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
        dag_id: &str,
    ) -> Result<Option<(DagSummary, Vec<Task>)>, AgentError> {
        for wid in workspace_ids.iter().copied() {
            let tasks = self.task_list(scope, wid)?;
            let Some(dag) = group_tasks_into_dags(&tasks)
                .into_iter()
                .find(|d| d.id == dag_id)
            else {
                continue;
            };
            let subset = tasks
                .into_iter()
                .filter(|t| dag.task_ids.contains(&t.id))
                .collect();
            return Ok(Some((dag, subset)));
        }
        Ok(None)
    }

    pub fn task_get(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(workspace_id, task_id)
        })
    }

    /// 러너가 TTL 을 갱신하지 못해 task 의 지금 회차가 점유를 잃은 기록. 없으면 빈 목록.
    pub fn task_holding_warnings(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Vec<serde_json::Value> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            crate::runner_host::current_holding_warnings(mem, seq.as_ref(), workspace_id, task_id)
        })
    }

    /// task 와 그 레코드 revision 을 한 번의 잠금 안에서 읽는다.
    pub fn task_get_with_revision(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<(Task, Option<u64>)>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            match store.get(workspace_id, task_id)? {
                Some(t) => Ok(Some((t, store.revision(workspace_id, task_id)?))),
                None => Ok(None),
            }
        })
    }

    pub fn task_cancel(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.cancel(workspace_id, task_id, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(scope, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(scope, workspace_id, d);
            }
            // 러너가 있으면 다음 tick 이 종결을 흡수해 프로세스를 끝내고 점유를 정리한다. 없으면
            // 지금 정리를 시작한다. 지켜보는 러너가 없어 취소한 Run 이 계속 실행되지 않게 하기
            // 위해서다. 종료 확인과 반환은 백그라운드에서 하고 응답은 기다리지 않는다.
            if !self.runner_registry().has_live_runner(workspace_id) {
                let tasks = std::iter::once(task).chain(downstream).cloned().collect();
                crate::runner_thread::settle_ended_tasks_in_background(
                    self.runner_context(scope),
                    workspace_id,
                    tasks,
                );
            }
        }
        result
    }

    /// 종결 상태 전이 경로는 대기자를 깨우고 사건 피드를 기록하도록 이 hub를 호출해야 한다.
    fn fire_waker_if_terminal(&self, scope: &TaskScope, workspace_id: u32, task: &Task) {
        if !task.state.is_terminal() {
            return;
        }
        let seq = scope.agent_seq().clone();
        let revision = self
            .with_memory(|mem| {
                TaskStore::new(mem, HOST_OWNER, seq.as_ref()).revision(workspace_id, &task.id)
            })
            .unwrap_or_else(|error| {
                tracing::warn!(%error, workspace_id, task_id = %task.id, "task revision lookup failed");
                None
            });
        scope.waker_hub().fire(
            workspace_id,
            &task.id,
            crate::task_waker::TerminalSnapshot::of(task, revision),
        );
    }

    pub fn task_retry(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.retry(workspace_id, task_id, reset_downstream, now_ms)
        })
    }

    /// 상태 변경과 자동 전이된 후속 작업을 반환한다.
    pub fn task_set_state(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_state(workspace_id, task_id, new_state, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(scope, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(scope, workspace_id, d);
            }
        }
        result
    }

    /// 외부 완료 보고. 결과와 종결을 한 번에 기록하고 이 보고로 종결된 task 의 대기자를 깨운다
    /// ([`TaskStore::complete`]).
    pub fn task_complete(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<CompletionReceipt, AgentError> {
        self.runner_context(scope)
            .complete_task(workspace_id, task_id, completion, now_ms)
    }

    /// 훅 매핑을 소비해 exit code가 0 또는 없으면 성공, 나머지는 실패로 처리한다.
    /// 등록된 원 hub/agent_seq를 사용한다. 소유 정보 없는 legacy 등록만 호출 scope를 사용한다.
    /// 매핑은 저장 전에 제거하며 저장 실패 때 다시 등록하지 않는다. 훅을 건 회차가 이미
    /// 끝났거나 바뀌었으면 보고를 적용하지 않는다.
    pub fn resolve_hook_task_wait(
        &self,
        scope: &TaskScope,
        hook_id: u64,
        exit_code: Option<i32>,
        now_ms: u64,
    ) {
        let Some(wait) = self.hook_task_waits().resolve_owned(hook_id) else {
            return;
        };
        let mut context = self.runner_context(scope);
        if let Some(owner) = wait.owner {
            context.agent_seq = owner.agent_seq;
            context.task_waker_hub = owner.completion;
        }
        let outcome = match exit_code {
            Some(code) if code != 0 => CompletionOutcome::Failed {
                error: format!("command exited with code {code}"),
            },
            _ => CompletionOutcome::Succeeded,
        };
        let completion = Completion {
            attempt_id: wait.attempt,
            result: TaskResult {
                exit_code,
                output: None,
                error: None,
            },
            outcome,
            postprocess: None,
            main_copy_dropped: false,
        };
        let task_id = wait.task;
        if let Err(error) = context.complete_task(wait.workspace, &task_id, completion, now_ms) {
            tracing::warn!(%error, %task_id, "resolve_hook_task_wait completion not recorded");
        }
    }

    /// 저장소 락 안에서는 입력 결과만 모으고 실제 reducer 실행은 호출자가 락 밖에서 한다.
    pub fn task_reduce_collect(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
            for tid in inputs {
                let task = match store.get(workspace_id, tid)? {
                    Some(t) => t,
                    None => return Err(AgentError::TaskNotFound(tid.clone())),
                };
                // 단발 reduce 도 v1 reducer 다. v2 결과는 같은 원칙으로 받지 않는다.
                if task.is_typed() {
                    return Err(typed_read_refused(
                        tid,
                        "is a typed v2 task; a v1 reduce cannot take it as an input",
                    ));
                }
                let succeeded = matches!(task.state, TaskState::Succeeded);
                let output = task
                    .result
                    .and_then(|r| r.output)
                    .unwrap_or(serde_json::Value::Null);
                out.push(ReducerInput {
                    succeeded,
                    task_id: tid.clone(),
                    output,
                });
            }
            Ok(out)
        })
    }

    /// 참조·Running 검사를 통과해 삭제된 작업의 handle·실행 결과도 정리한다.
    /// 부속 키 정리는 저장소 락을 다시 사용하므로 첫 락을 놓은 뒤 호출해야 한다.
    pub fn task_delete(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport, AgentError> {
        let seq = scope.agent_seq().clone();
        let report = self.with_memory(|mem| {
            let settling = crate::runner_thread::stored_handle_ids(&*mem, workspace_id);
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            reject_settling_targets(&store, workspace_id, task_id, opts.cascade, &settling)?;
            store.delete_checked(workspace_id, task_id, opts)
        })?;
        let ctx = self.runner_context(scope);
        for id in &report.deleted {
            evict_task_side_keys(&ctx, workspace_id, id);
        }
        Ok(report)
    }

    /// 상태나 경과시간 조건 중 하나는 있어야 한다. dry_run과 실제 삭제가 같은 계획을 사용한다.
    /// 계획 조회와 적용은 별도 락 구간이다.
    pub fn task_purge(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        filter: TaskPurgeFilter,
        dry_run: bool,
    ) -> Result<TaskSweepPlan, AgentError> {
        if filter.states.is_none() && filter.older_than_ms.is_none() {
            return Err(AgentError::InvalidArgument(
                "task_purge requires at least one of 'states'/'older_than_ms'".into(),
            ));
        }
        let seq = scope.agent_seq().clone();
        // 이전 회차의 종료를 확인하는 중인 task 는 건너뛴다(`skipped`). 지우면 점유를 반환할 정리가 없어진다.
        let plan = self.with_memory(|mem| {
            let keep = crate::runner_thread::stored_handle_ids(&*mem, workspace_id);
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.plan_sweep(workspace_id, &filter, &keep)
        })?;
        if dry_run {
            return Ok(plan);
        }
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.apply_sweep_plan(workspace_id, &plan)
        })?;
        let ctx = self.runner_context(scope);
        for id in &plan.deleted {
            evict_task_side_keys(&ctx, workspace_id, id);
        }
        Ok(plan)
    }
}

/// 서비스와 서비스를 받지 못하는 화면이 같은 목록 조회를 쓴다. 화면은 engine의 저장소를 넘긴다.
pub fn task_list_from_state(
    memory: &std::sync::Mutex<dyn tasty_memory::MemoryStorage>,
    scope: &TaskScope,
    workspace_id: u32,
) -> Result<Vec<Task>, AgentError> {
    let mut guard = tasty_utils::poison::recover_mutex(
        memory.lock(),
        tasty_memory::STORE_LOCK_WHAT,
        &tasty_memory::STORE_LOCK_POISONED,
    );
    let store = TaskStore::new(&mut *guard, HOST_OWNER, scope.agent_seq().as_ref());
    store.list(workspace_id)
}

/// 받은 workspace만 순회한다. 호출자는 dag_scan_workspaces로 engine의 live workspace를 넘겨
/// 삭제된 workspace의 고아 scope를 조회하지 않게 한다.
pub fn dag_list_from_state(
    memory: &std::sync::Mutex<dyn tasty_memory::MemoryStorage>,
    scope: &TaskScope,
    workspace_ids: &[u32],
) -> Result<Vec<DagSummary>, AgentError> {
    let mut out = Vec::new();
    for wid in workspace_ids {
        out.extend(group_tasks_into_dags(&task_list_from_state(
            memory, scope, *wid,
        )?));
    }
    Ok(out)
}

/// v1 이 v2 task 의 결과를 읽는 경로를 생성 단계에서 거절한다. 출력 placeholder
/// (`${task.<id>.output…}`)와 v1 `Reduce.inputs` 는 v1 결과(`result.output`)를 읽는데,
/// v2 결과는 계약으로 타입이 정해진 값(run 은 종료 코드)이라 v1 으로 넘기면 의미가
/// 조용히 바뀐다. 어느 범위를 허용할지는 입력 binding 이 정한다.
pub(crate) fn reject_v1_reads_of_typed(
    store: &TaskStore,
    workspace_id: u32,
    command: &tasty_agent::TaskCommand,
    on_failure: &tasty_agent::OnFailure,
) -> Result<(), AgentError> {
    // 문법 오류는 IPC 검사와 실행 직전 치환이 보고한다.
    for tid in crate::task_output_ref::referenced_tasks(command).unwrap_or_default() {
        if store.get(workspace_id, &tid)?.is_some_and(|t| t.is_typed()) {
            return Err(typed_read_refused(
                &tid,
                "points to a typed v2 task; ${task.<id>.output} placeholders read v1 results only",
            ));
        }
    }
    if let tasty_agent::TaskCommand::Reduce { inputs, .. } = command {
        for tid in inputs {
            if store.get(workspace_id, tid)?.is_some_and(|t| t.is_typed()) {
                return Err(typed_read_refused(
                    tid,
                    "is a typed v2 task; a v1 reduce cannot take it as an input",
                ));
            }
        }
    }
    if let tasty_agent::OnFailure::Fallback {
        inline: Some(spec), ..
    } = on_failure
    {
        reject_v1_reads_of_typed(store, workspace_id, &spec.command, &spec.on_failure)?;
    }
    Ok(())
}

/// [`TaskService::task_graph_submit`] 의 결과. `dry_run` 이면 `activated` 가 false 이고
/// `tasks` 는 저장하지 않은 검증 결과다.
#[derive(Debug, Clone)]
pub struct GraphSubmitOutcome {
    pub graph_id: String,
    pub activated: bool,
    pub tasks: Vec<Task>,
}

/// 그래프로 제출한 v2 task 는 값을 binding 으로만 받는다. 문자열 placeholder 는 값을 다시
/// 해석하는 경로라 받지 않는다.
fn reject_output_placeholders(index: usize, task: &Task) -> Result<(), AgentError> {
    use tasty_agent::task::contract::{FailureStage, TaskFailure};
    let refs = crate::task_output_ref::referenced_tasks(&task.command);
    let message = match refs {
        Ok(ids) if ids.is_empty() => return Ok(()),
        Ok(ids) => format!(
            "task {}: output placeholders (${{task.<id>.output}}) are not read in a typed graph; \
             bind {} through 'bindings' and 'input_mapping'",
            task.id,
            ids.into_iter().collect::<Vec<_>>().join(", ")
        ),
        Err(e) => format!("task {}: {}", task.id, e.0),
    };
    let mut failure = TaskFailure::new(FailureStage::Input, message);
    failure.task_id = Some(task.id.clone());
    failure.location = Some(format!("/tasks/{index}/command"));
    Err(AgentError::TypeContract(Box::new(failure)))
}

fn typed_read_refused(tid: &TaskId, why: &str) -> AgentError {
    use tasty_agent::task::contract::{FailureStage, TaskFailure};
    let mut failure = TaskFailure::new(FailureStage::Input, format!("task '{tid}' {why}"));
    failure.task_id = Some(tid.clone());
    AgentError::TypeContract(Box::new(failure))
}

/// 러너가 task 에 다는 단계 표지. `Task::phase` 에 더해, handle 이 남은 Ready task 는 이전 회차의
/// 프로세스가 끝난 것을 확인할 때까지 시작하지 않으므로 [`WAITING_PREVIOUS_ATTEMPT`] 이다.
pub fn task_phase(task: &Task, stored_handles: &HashSet<TaskId>) -> Option<&'static str> {
    task_phase_with_handle(task, stored_handles.contains(&task.id))
}

/// [`task_phase`] 의 task 한 건 판. handle 이 남았는지(`has_stored_handle`)를 직접 받는다.
pub fn task_phase_with_handle(task: &Task, has_stored_handle: bool) -> Option<&'static str> {
    if matches!(task.state, TaskState::Ready) && has_stored_handle {
        return Some(WAITING_PREVIOUS_ATTEMPT);
    }
    task.phase()
}

/// 이전 회차의 종료 확인을 기다리는 Ready task 의 단계 이름.
pub const WAITING_PREVIOUS_ATTEMPT: &str = "waiting_previous_attempt";

impl TaskService {
    /// handle 이 남은 task id(이전 회차 종료 확인 중이거나 실행 중). [`task_phase`] 에 넘긴다.
    pub fn stored_handle_ids(&self, workspace_id: u32) -> HashSet<TaskId> {
        self.with_memory(|mem| crate::runner_thread::stored_handle_ids(&*mem, workspace_id))
    }

    /// task 한 건의 handle 이 남았는가. 목록을 읽는 [`Self::stored_handle_ids`] 와 달리 키 하나만
    /// 읽는다([`crate::runner_host::has_stored_handle`]).
    pub fn has_stored_handle(&self, workspace_id: u32, task_id: &TaskId) -> bool {
        self.with_memory(|mem| crate::runner_host::has_stored_handle(&*mem, workspace_id, task_id))
    }
}

/// 지울 task(cascade 면 그 참조자 포함) 중 Running 이 아닌데 handle 이 남은 것이 있으면 거절한다.
/// 그 task 는 이전 회차의 프로세스가 끝난 것을 아직 확인하지 못했다. 지우면 handle 이 사라져 확인 뒤
/// semaphore·lease 를 반환할 정리가 없어진다. Running 은 `delete_checked` 가 거절한다.
fn reject_settling_targets(
    store: &TaskStore<'_>,
    workspace_id: u32,
    task_id: &TaskId,
    cascade: bool,
    settling: &HashSet<TaskId>,
) -> Result<(), AgentError> {
    if settling.is_empty() {
        return Ok(());
    }
    let all = store.list(workspace_id)?;
    let mut targets = vec![task_id.clone()];
    if cascade {
        targets.extend(tasty_agent::task::transitive_referencing_task_ids(
            &all, task_id,
        ));
    }
    let blocked = all.iter().find(|t| {
        targets.contains(&t.id)
            && settling.contains(&t.id)
            && !matches!(t.state, TaskState::Running)
    });
    match blocked {
        Some(t) => Err(AgentError::InvalidArgument(format!(
            "task {} is waiting for exit confirmation: the processes of its previous attempt have not been confirmed ended, and its semaphore and lease are returned after that; delete it once they end",
            t.id
        ))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn task(command: serde_json::Value) -> Task {
        serde_json::from_value(json!({
            "id": "c", "workspace_id": 1, "name": "c", "command": command,
            "state": {"kind": "waiting"}, "created_at": 0
        }))
        .expect("task")
    }

    #[test]
    fn typed_graph_tasks_refuse_output_placeholders_with_a_location() {
        let ok = task(json!({"kind": "custom", "ipc_method": "system.ping",
                             "params": {"cwd": "${lease.resource}"}}));
        assert!(reject_output_placeholders(0, &ok).is_ok());
        let bad = task(json!({"kind": "run", "workspace_id": 1,
                              "command": ["echo", "${task.p.output/x}"]}));
        let Err(AgentError::TypeContract(f)) = reject_output_placeholders(3, &bad) else {
            panic!("expected a refusal");
        };
        assert_eq!(f.location.as_deref(), Some("/tasks/3/command"));
        assert!(f.message.contains("bindings"), "{}", f.message);
        let malformed = task(json!({"kind": "run", "workspace_id": 1,
                                    "command": ["echo", "${task.x}"]}));
        assert!(reject_output_placeholders(0, &malformed).is_err());
    }

    fn service(fallback: Option<&str>) -> (TaskService, TaskScope) {
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().expect("memory"),
            ));
        let svc = TaskService::new(
            memory,
            std::sync::Arc::new(std::sync::OnceLock::new()),
            std::sync::Arc::new(crate::completion::fixture::Resolver::default()),
        )
        .with_store_fallback(fallback.map(str::to_string));
        let scope = TaskScope::new(svc.runner_registry().clone());
        (svc, scope)
    }

    fn graph(durability: Option<&str>) -> TaskGraphSpec {
        let mut g = json!({"contract_version": 2, "tasks": [
            {"id": "only", "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}}}]});
        if let Some(d) = durability {
            g["durability"] = json!(d);
        }
        serde_json::from_value(g).expect("graph")
    }

    #[test]
    fn a_direct_caller_gets_the_same_durability_judgement_as_ipc() {
        let (svc, scope) = service(Some("corrupt"));
        assert!(!svc.store_durable());
        for dry_run in [true, false] {
            let e = svc
                .task_graph_submit(&scope, 1, graph(None), dry_run, 0)
                .expect_err("required graph on a fallback store");
            assert!(
                matches!(&e, AgentError::StoreNotDurable { cause } if cause == "corrupt"),
                "{e:?}"
            );
        }
        assert!(svc.task_list(&scope, 1).unwrap().is_empty());
        let ok = svc
            .task_graph_submit(&scope, 1, graph(Some("best_effort")), false, 0)
            .expect("best effort");
        assert!(ok.activated);

        let (durable, scope) = service(None);
        assert!(durable.store_durable());
        durable
            .task_graph_submit(&scope, 1, graph(None), false, 0)
            .expect("durable store");
    }
}
