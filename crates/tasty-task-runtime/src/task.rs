//! TaskService의 작업 API. 원본은 memory의 TaskStore이며 engine별 순번·허브는 TaskScope로 받는다.

use tasty_agent::task::{
    TaskCreateOpts, TaskDeleteOpts, TaskDeleteReport, TaskPurgeFilter, TaskSweepPlan,
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
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            reject_output_refs_to_typed(
                &store,
                opts.workspace_id,
                &opts.command,
                &opts.on_failure,
            )?;
            if reserved_for_fallback {
                store.create_reserved_for_fallback(opts)
            } else {
                store.create(opts)
            }
        })
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
        }
        result
    }

    /// 종결 상태 전이 경로는 대기자를 깨우고 사건 피드를 기록하도록 이 hub를 호출해야 한다.
    fn fire_waker_if_terminal(&self, scope: &TaskScope, workspace_id: u32, task: &Task) {
        if !task.state.is_terminal() {
            return;
        }
        scope.waker_hub().fire(
            workspace_id,
            &task.id,
            crate::task_waker::TerminalSnapshot {
                state: task.state.clone(),
                result: task.result.clone(),
            },
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

    pub fn task_set_result(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        result: TaskResult,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_result(workspace_id, task_id, result)
        })
    }

    /// 훅 매핑을 소비해 exit code가 0 또는 없으면 성공, 나머지는 실패로 처리한다.
    /// 등록된 원 hub/agent_seq를 사용한다. 소유 정보 없는 legacy 등록만 호출 scope를 사용한다.
    /// 매핑은 저장 전에 제거하며 저장 실패 때 다시 등록하지 않는다.
    pub fn resolve_hook_task_wait(
        &self,
        scope: &TaskScope,
        hook_id: u64,
        exit_code: Option<i32>,
        now_ms: u64,
    ) {
        let Some((workspace_id, task_id, owner)) = self.hook_task_waits().resolve_owned(hook_id)
        else {
            return;
        };
        let mut context = self.runner_context(scope);
        if let Some(owner) = owner {
            context.agent_seq = owner.agent_seq;
            context.task_waker_hub = owner.completion;
        }
        let result = TaskResult {
            exit_code,
            output: None,
            error: None,
        };
        if let Err(e) = context.with_memory(|memory| {
            TaskStore::new(memory, HOST_OWNER, context.agent_seq.as_ref()).set_result(
                workspace_id,
                &task_id,
                result,
            )
        }) {
            tracing::warn!("resolve_hook_task_wait: set_result {task_id} failed: {e}");
            return;
        }
        let new_state = match exit_code {
            Some(code) if code != 0 => TaskState::Failed {
                error: format!("command exited with code {code}"),
            },
            _ => TaskState::Succeeded,
        };
        let transitioned = context.with_memory(|memory| {
            TaskStore::new(memory, HOST_OWNER, context.agent_seq.as_ref()).set_state(
                workspace_id,
                &task_id,
                new_state,
                now_ms,
            )
        });
        match transitioned {
            Ok((task, downstream)) => {
                context.fire_terminal_tasks(workspace_id, std::iter::once(task).chain(downstream))
            }
            Err(error) => {
                tracing::warn!(%error,%task_id,"resolve_hook_task_wait state change failed")
            }
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
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
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
        let plan = self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.plan_sweep(workspace_id, &filter)
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

/// `${task.<id>.output…}` 는 v1 결과(`result.output`)를 읽는다. v2 task 의 결과는 타입이 있는
/// wire 값(int64 는 10진 문자열, run 은 종료 코드)이라 v1 참조로 넘기면 의미가 조용히 바뀐다.
/// 입력 binding 이 그 경로를 정하기 전까지 생성 단계에서 거절한다.
pub(crate) fn reject_output_refs_to_typed(
    store: &TaskStore,
    workspace_id: u32,
    command: &tasty_agent::TaskCommand,
    on_failure: &tasty_agent::OnFailure,
) -> Result<(), AgentError> {
    use tasty_agent::task::contract::{FailureStage, TaskFailure};
    // 문법 오류는 IPC 검사와 실행 직전 치환이 보고한다.
    for tid in crate::task_output_ref::referenced_tasks(command).unwrap_or_default() {
        if store.get(workspace_id, &tid)?.is_some_and(|t| t.is_typed()) {
            let mut failure = TaskFailure::new(
                FailureStage::Input,
                format!(
                    "task output reference '{tid}' points to a typed v2 task; \
                     ${{task.<id>.output}} placeholders read v1 results only"
                ),
            );
            failure.task_id = Some(tid);
            return Err(AgentError::TypeContract(Box::new(failure)));
        }
    }
    if let tasty_agent::OnFailure::Fallback {
        inline: Some(spec), ..
    } = on_failure
    {
        reject_output_refs_to_typed(store, workspace_id, &spec.command, &spec.on_failure)?;
    }
    Ok(())
}
