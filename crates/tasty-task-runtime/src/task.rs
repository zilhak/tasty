//! TaskService의 작업 API. 원본은 memory의 TaskStore이며 engine별 순번·허브는 TaskScope로 받는다.

use std::collections::HashSet;

use tasty_agent::task::{
    Completion, CompletionOutcome, CompletionReceipt, TaskCreateOpts, TaskDeleteOpts,
    TaskDeleteReport, TaskGraphSpec, TaskPurgeFilter, TaskSweepPlan,
};
use tasty_agent::{
    AgentError, DagSummary, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore,
    group_tasks_into_dags,
};
use tasty_memory::HOST_OWNER;

use crate::task_tx::TaskTx;
use crate::{TaskScope, TaskService};

impl TaskService {
    /// fallback 예약 작업은 참조할 본 작업이 등록되기 전에 Ready가 되지 않게 만든다.
    pub fn task_create(
        &self,
        scope: &TaskScope,
        opts: TaskCreateOpts,
        reserved_for_fallback: bool,
    ) -> Result<Task, AgentError> {
        let scope = scope.clone();
        self.transact(move |tx| tx.task_create(&scope, opts.clone(), reserved_for_fallback))
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
        let scope = scope.clone();
        self.transact(move |tx| {
            tx.task_graph_submit(&scope, workspace_id, spec.clone(), dry_run, now_ms)
        })
    }

    pub fn task_list(&self, scope: &TaskScope, workspace_id: u32) -> Result<Vec<Task>, AgentError> {
        let scope = scope.clone();
        self.transact(move |tx| tx.task_list(&scope, workspace_id))
    }

    /// 받은 workspace만 순회한다. 화면의 DAG 목록과 같은 구현을 쓴다.
    pub fn dag_list(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
    ) -> Result<Vec<DagSummary>, AgentError> {
        let (scope, workspace_ids) = (scope.clone(), workspace_ids.to_vec());
        self.transact(move |tx| tx.dag_list(&scope, &workspace_ids))
    }

    /// 받은 순서의 첫 일치를 반환한다. dag_scan_workspaces는 ID 오름차순으로 넘긴다.
    /// 사용자가 정한 DAG 키가 여러 workspace에 같을 수 있어 구별하려면 workspace_id도 지정한다.
    pub fn dag_get(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
        dag_id: &str,
    ) -> Result<Option<(DagSummary, Vec<Task>)>, AgentError> {
        let (scope, workspace_ids, dag_id) =
            (scope.clone(), workspace_ids.to_vec(), dag_id.to_string());
        self.transact(move |tx| tx.dag_get(&scope, &workspace_ids, &dag_id))
    }

    pub fn task_get(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| tx.task_get(&scope, workspace_id, &task_id))
    }

    /// 러너가 TTL 을 갱신하지 못해 task 의 지금 회차가 점유를 잃은 기록. 없으면 빈 목록.
    pub fn task_holding_warnings(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Vec<serde_json::Value> {
        let (svc, scope, id) = (self.clone(), scope.clone(), task_id.clone());
        self.memory_access()
            .run_blocking(0, move |mem| {
                TaskTx::new(mem, &svc).task_holding_warnings(&scope, workspace_id, &id)
            })
            .unwrap_or_else(|error| {
                tracing::warn!(%error, %task_id, "holding warnings not read");
                Vec::new()
            })
    }

    /// task 와 그 레코드 revision 을 같은 writer 요청 안에서 읽는다.
    pub fn task_get_with_revision(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<(Task, Option<u64>)>, AgentError> {
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| tx.task_get_with_revision(&scope, workspace_id, &task_id))
    }

    pub fn task_cancel(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| tx.task_cancel(&scope, workspace_id, &task_id, now_ms))
    }

    pub fn task_retry(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| {
            tx.task_retry(&scope, workspace_id, &task_id, reset_downstream, now_ms)
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
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| {
            tx.task_set_state(&scope, workspace_id, &task_id, new_state.clone(), now_ms)
        })
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
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| {
            tx.task_complete(&scope, workspace_id, &task_id, completion.clone(), now_ms)
        })
    }

    /// 훅 매핑을 소비해 exit code가 0 또는 없으면 성공, 나머지는 실패로 처리한다.
    /// 등록된 원 hub/agent_seq를 사용한다. 소유 정보 없는 legacy 등록만 호출 scope를 사용한다.
    /// 매핑은 저장 전에 제거하며 저장 실패 때 다시 등록하지 않는다. 훅을 건 회차가 이미
    /// 끝났거나 바뀌었으면 보고를 적용하지 않는다.
    ///
    /// 메인 루프가 부른다. 완료 기록은 기다리지 않는 writer 요청이고, 대기자 깨우기는 그 요청이
    /// 확정된 뒤 메인 루프가 결과를 거둘 때 한다. 기록하지 못하면 로그만 남긴다.
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
        let (seq, hub) = match wait.owner {
            Some(owner) => (owner.agent_seq, owner.completion),
            None => (scope.agent_seq().clone(), scope.waker_hub().clone()),
        };
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
        let workspace_id = wait.workspace;
        let request = self.unit_request(0, move |tx| {
            tx.complete_with(
                &seq,
                &hub,
                workspace_id,
                &task_id,
                completion.clone(),
                now_ms,
            )
            .map_err(|error| (error.to_string(), task_id.clone()))
        });
        self.memory_access().submit_detached(
            request,
            "hook task completion",
            Some(Box::new(|output| {
                match output.into_value::<(
                    Result<CompletionReceipt, (String, TaskId)>,
                    crate::task_tx::TaskEffects,
                )>() {
                    Some((Ok(_), effects)) => effects.apply(),
                    Some((Err((error, task_id)), effects)) => {
                        effects.apply();
                        tracing::warn!(%error, %task_id, "resolve_hook_task_wait completion not recorded");
                    }
                    None => tracing::error!("hook task completion returned another value"),
                }
            })),
        );
    }

    /// 저장소 잠금 안에서는 입력 결과만 모으고 실제 reducer 실행은 호출자가 락 밖에서 한다.
    pub fn task_reduce_collect(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let (scope, inputs) = (scope.clone(), inputs.to_vec());
        self.transact(move |tx| tx.task_reduce_collect(&scope, workspace_id, &inputs))
    }

    /// 참조·Running 검사를 통과해 삭제된 작업의 handle·실행 결과도 정리한다.
    pub fn task_delete(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport, AgentError> {
        let (scope, task_id) = (scope.clone(), task_id.clone());
        self.transact(move |tx| tx.task_delete(&scope, workspace_id, &task_id, opts))
    }

    /// 상태나 경과시간 조건 중 하나는 있어야 한다. dry_run과 실제 삭제가 같은 계획을 사용한다.
    pub fn task_purge(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        filter: TaskPurgeFilter,
        dry_run: bool,
    ) -> Result<TaskSweepPlan, AgentError> {
        let scope = scope.clone();
        self.transact(move |tx| tx.task_purge(&scope, workspace_id, filter.clone(), dry_run))
    }
}

/// 한 workspace의 task 목록. 서비스와 서비스를 받지 못하는 화면이 같은 조회를 쓴다.
pub fn task_list_in(
    mem: &mut dyn tasty_memory::MemoryStorage,
    seq: &std::sync::atomic::AtomicU64,
    workspace_id: u32,
) -> Result<Vec<Task>, AgentError> {
    TaskStore::new(mem, HOST_OWNER, seq).list(workspace_id)
}

/// 받은 workspace만 순회한다. 호출자는 dag_scan_workspaces로 engine의 live workspace를 넘겨
/// 삭제된 workspace의 고아 scope를 조회하지 않게 한다.
pub fn dag_list_in(
    mem: &mut dyn tasty_memory::MemoryStorage,
    seq: &std::sync::atomic::AtomicU64,
    workspace_ids: &[u32],
) -> Result<Vec<DagSummary>, AgentError> {
    let mut out = Vec::new();
    for wid in workspace_ids {
        out.extend(group_tasks_into_dags(&task_list_in(&mut *mem, seq, *wid)?));
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
pub(crate) fn reject_output_placeholders(index: usize, task: &Task) -> Result<(), AgentError> {
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

pub(crate) fn typed_read_refused(tid: &TaskId, why: &str) -> AgentError {
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
    /// 읽지 못하면 경고를 남기고 빈 목록이다.
    pub fn stored_handle_ids(&self, workspace_id: u32) -> HashSet<TaskId> {
        self.memory_access()
            .run_blocking(0, move |mem| {
                crate::runner_thread::stored_handle_ids(&*mem, workspace_id)
            })
            .unwrap_or_else(|error| {
                tracing::warn!(%error, workspace_id, "stored handles not read");
                HashSet::new()
            })
    }

    /// task 한 건의 handle 이 남았는가. 목록을 읽는 [`Self::stored_handle_ids`] 와 달리 키 하나만
    /// 읽는다([`crate::runner_host::has_stored_handle`]). 읽지 못하면 경고를 남기고 남지 않은 것으로 본다.
    pub fn has_stored_handle(&self, workspace_id: u32, task_id: &TaskId) -> bool {
        let id = task_id.clone();
        self.memory_access()
            .run_blocking(0, move |mem| {
                crate::runner_host::has_stored_handle(&*mem, workspace_id, &id)
            })
            .unwrap_or_else(|error| {
                tracing::warn!(%error, workspace_id, %task_id, "stored handle not read");
                false
            })
    }
}

/// 지울 task(cascade 면 그 참조자 포함) 중 Running 이 아닌데 handle 이 남은 것이 있으면 거절한다.
/// 그 task 는 이전 회차의 프로세스가 끝난 것을 아직 확인하지 못했다. 지우면 handle 이 사라져 확인 뒤
/// semaphore·lease 를 반환할 정리가 없어진다. Running 은 `delete_checked` 가 거절한다.
pub(crate) fn reject_settling_targets(
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
            tasty_memory::writer::MemoryAccess::inline(memory),
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
