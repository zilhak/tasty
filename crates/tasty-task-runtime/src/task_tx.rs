//! 저장소 하나를 받아 task 연산을 실행하는 단위. memory writer의 요청 본문 안에서 만든다.
//!
//! 연산의 판단과 쓰기는 이 단위가 받은 저장소에서 한 번에 한다. 대기자 깨우기·runner 정리 시작·
//! 부속 키 정리처럼 commit 뒤에 해야 하는 일은 [`TaskEffects`]에 모으고, 호출자가 commit을 확인한
//! 뒤 [`TaskEffects::apply`]로 실행한다. writer는 본문을 다시 실행할 수 있으므로 본문은 효과를 직접
//! 내지 않는다. IPC는 writer 요청 하나와 continuation으로, runner·worker·시험은
//! [`TaskService`]의 기다리는 메서드로 같은 연산을 쓴다.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use tasty_agent::task::report::{AppendOutcome, ReportAddress, project_task};
use tasty_agent::task::{
    Completion, CompletionReceipt, GraphDurability, TaskCreateOpts, TaskDeleteOpts,
    TaskDeleteReport, TaskGraphSpec, TaskPurgeFilter, TaskSweepPlan,
};
use tasty_agent::{
    AgentError, DagSummary, ReducerInput, Task, TaskId, TaskState, TaskStore, group_tasks_into_dags,
};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use crate::agent_task::Submitter;
use crate::runner_host::RunnerContext;
use crate::runner_thread::RunnerStatus;
use crate::task::GraphSubmitOutcome;
use crate::task_waker::{TaskWakerHub, TerminalSnapshot};
use crate::{TaskScope, TaskService};

/// commit 뒤에 실행할 일. 본문이 다시 실행되면 이전 실행의 효과는 버려진다.
#[derive(Default)]
#[must_use = "effects must be applied after the commit"]
pub struct TaskEffects(Vec<TaskEffect>);

enum TaskEffect {
    /// 종결된 task의 대기자를 깨운다. revision은 같은 단위 안에서 읽은 값이다.
    Fire {
        hub: Arc<TaskWakerHub>,
        workspace_id: u32,
        task: Box<Task>,
        revision: Option<u64>,
    },
    /// 지켜보는 runner가 없는 workspace의 종결 task를 백그라운드에서 정리한다.
    Settle {
        ctx: Box<RunnerContext>,
        workspace_id: u32,
        tasks: Vec<Task>,
    },
    /// 삭제된 task의 handle·실행 결과·점유 경고를 지운다. 기다리지 않는 writer 요청이며 실패는
    /// 로그만 남긴다(삭제 자체를 되돌리지 않는다).
    EvictSideKeys {
        access: tasty_memory::writer::MemoryAccess,
        workspace_id: u32,
        task_ids: Vec<TaskId>,
    },
}

impl TaskEffects {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// commit을 확인한 뒤 부른다. 메인 루프에서도 기다리지 않는다.
    pub fn apply(self) {
        for effect in self.0 {
            match effect {
                TaskEffect::Fire {
                    hub,
                    workspace_id,
                    task,
                    revision,
                } => hub.fire(
                    workspace_id,
                    &task.id,
                    TerminalSnapshot::of(&task, revision),
                ),
                TaskEffect::Settle {
                    ctx,
                    workspace_id,
                    tasks,
                } => crate::runner_thread::settle_ended_tasks_in_background(
                    *ctx,
                    workspace_id,
                    tasks,
                ),
                TaskEffect::EvictSideKeys {
                    access,
                    workspace_id,
                    task_ids,
                } => access.submit_detached(
                    tasty_memory::writer::MemoryRequest::unit(0, move |mem| {
                        for id in &task_ids {
                            crate::runner_host::evict_task_side_keys_in(mem, workspace_id, id);
                        }
                        Ok(())
                    }),
                    "task side key eviction",
                    None,
                ),
            }
        }
    }
}

/// 저장소 하나에서 task 연산을 실행한다. [`TaskService`]의 같은 이름 메서드와 같은 판정을 한다.
pub struct TaskTx<'a> {
    mem: &'a mut dyn MemoryStorage,
    svc: &'a TaskService,
    effects: TaskEffects,
}

impl<'a> TaskTx<'a> {
    pub fn new(mem: &'a mut dyn MemoryStorage, svc: &'a TaskService) -> Self {
        Self {
            mem,
            svc,
            effects: TaskEffects::default(),
        }
    }

    /// 모은 효과. 호출자가 commit 뒤 [`TaskEffects::apply`]한다.
    pub fn into_effects(self) -> TaskEffects {
        self.effects
    }

    fn store<'s>(&'s mut self, seq: &'s AtomicU64) -> TaskStore<'s> {
        TaskStore::new(&mut *self.mem, HOST_OWNER, seq)
    }

    /// 종결된 task면 같은 단위에서 revision을 읽어 commit 뒤 깨우기를 예약한다.
    pub(crate) fn fire_if_terminal(
        &mut self,
        seq: &AtomicU64,
        hub: &Arc<TaskWakerHub>,
        workspace_id: u32,
        task: &Task,
    ) {
        if !task.state.is_terminal() {
            return;
        }
        let revision = self
            .store(seq)
            .revision(workspace_id, &task.id)
            .unwrap_or_else(|error| {
                tracing::warn!(%error, workspace_id, task_id = %task.id, "task revision lookup failed");
                None
            });
        self.effects.0.push(TaskEffect::Fire {
            hub: hub.clone(),
            workspace_id,
            task: Box::new(task.clone()),
            revision,
        });
    }

    fn fire_all<'t>(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        tasks: impl IntoIterator<Item = &'t Task>,
    ) {
        let seq = scope.agent_seq().clone();
        let hub = scope.waker_hub().clone();
        for task in tasks {
            self.fire_if_terminal(&seq, &hub, workspace_id, task);
        }
    }

    /// fallback 예약 작업은 참조할 본 작업이 등록되기 전에 Ready가 되지 않게 만든다.
    pub fn task_create(
        &mut self,
        scope: &TaskScope,
        opts: TaskCreateOpts,
        reserved_for_fallback: bool,
    ) -> Result<Task, AgentError> {
        crate::runner_host::check_holding_ttls(&opts.name, &opts.metadata, &opts.on_failure)
            .map_err(AgentError::InvalidArgument)?;
        let seq = scope.agent_seq().clone();
        let mut store = self.store(&seq);
        crate::task::reject_v1_reads_of_typed(
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
    }

    /// v2 task 그래프를 전체 검증하고, `dry_run` 이 아니면 저장·활성화한다. 검증에 실패하면
    /// 아무것도 저장하지 않는다.
    pub fn task_graph_submit(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        spec: TaskGraphSpec,
        dry_run: bool,
        now_ms: u64,
    ) -> Result<GraphSubmitOutcome, AgentError> {
        if let Some(cause) = self.svc.store_fallback()
            && spec.durability == GraphDurability::Required
        {
            return Err(AgentError::StoreNotDurable {
                cause: cause.to_string(),
            });
        }
        let seq = scope.agent_seq().clone();
        let outcome = {
            let mut store = self.store(&seq);
            let plan = store.plan_graph(workspace_id, spec, now_ms)?;
            for (i, t) in plan.tasks.iter().enumerate() {
                crate::task::reject_output_placeholders(i, t)?;
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
            GraphSubmitOutcome {
                graph_id: plan.graph_id,
                activated: true,
                tasks,
            }
        };
        self.fire_all(scope, workspace_id, &outcome.tasks);
        Ok(outcome)
    }

    pub fn task_list(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
    ) -> Result<Vec<Task>, AgentError> {
        crate::task::task_list_in(&mut *self.mem, scope.agent_seq(), workspace_id)
    }

    /// 받은 workspace만 순회한다. 화면의 DAG 목록과 같은 구현을 쓴다.
    pub fn dag_list(
        &mut self,
        scope: &TaskScope,
        workspace_ids: &[u32],
    ) -> Result<Vec<DagSummary>, AgentError> {
        crate::task::dag_list_in(&mut *self.mem, scope.agent_seq(), workspace_ids)
    }

    /// 받은 순서의 첫 일치를 반환한다.
    pub fn dag_get(
        &mut self,
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
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let seq = scope.agent_seq().clone();
        self.store(&seq).get(workspace_id, task_id)
    }

    /// 러너가 TTL 을 갱신하지 못해 task 의 지금 회차가 점유를 잃은 기록. 없으면 빈 목록.
    pub fn task_holding_warnings(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Vec<serde_json::Value> {
        crate::runner_host::current_holding_warnings(
            &mut *self.mem,
            scope.agent_seq().as_ref(),
            workspace_id,
            task_id,
        )
    }

    /// task 와 그 레코드 revision 을 같은 단위에서 읽는다.
    pub fn task_get_with_revision(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<(Task, Option<u64>)>, AgentError> {
        let seq = scope.agent_seq().clone();
        let store = self.store(&seq);
        match store.get(workspace_id, task_id)? {
            Some(t) => Ok(Some((t, store.revision(workspace_id, task_id)?))),
            None => Ok(None),
        }
    }

    /// 취소한 task와 함께 취소된 하류를 반환한다. 지켜보는 runner가 없으면 commit 뒤 정리를 시작한다.
    pub fn task_cancel(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let (task, downstream) = self.store(&seq).cancel(workspace_id, task_id, now_ms)?;
        self.fire_all(
            scope,
            workspace_id,
            std::iter::once(&task).chain(&downstream),
        );
        // 러너가 있으면 다음 tick 이 종결을 흡수해 프로세스를 끝내고 점유를 정리한다. 없으면
        // commit 뒤 정리를 시작한다. 지켜보는 러너가 없어 취소한 Run 이 계속 실행되지 않게 하기
        // 위해서다. 종료 확인과 반환은 백그라운드에서 하고 응답은 기다리지 않는다.
        if !self.svc.runner_registry().has_live_runner(workspace_id) {
            self.effects.0.push(TaskEffect::Settle {
                ctx: Box::new(self.svc.runner_context(scope)),
                workspace_id,
                tasks: std::iter::once(&task).chain(&downstream).cloned().collect(),
            });
        }
        Ok((task, downstream))
    }

    pub fn task_retry(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let seq = scope.agent_seq().clone();
        self.store(&seq)
            .retry(workspace_id, task_id, reset_downstream, now_ms)
    }

    /// 상태 변경과 자동 전이된 후속 작업을 반환한다.
    pub fn task_set_state(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = scope.agent_seq().clone();
        let (task, downstream) =
            self.store(&seq)
                .set_state(workspace_id, task_id, new_state, now_ms)?;
        self.fire_all(
            scope,
            workspace_id,
            std::iter::once(&task).chain(&downstream),
        );
        Ok((task, downstream))
    }

    /// 외부 완료 보고. 결과와 종결을 한 번에 기록하고 이 보고로 종결된 task 의 대기자를 깨운다.
    pub fn task_complete(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<CompletionReceipt, AgentError> {
        let seq = scope.agent_seq().clone();
        let hub = scope.waker_hub().clone();
        self.complete_with(&seq, &hub, workspace_id, task_id, completion, now_ms)
    }

    /// 완료 보고를 받은 순번·허브로 기록한다. 훅 완료는 훅을 건 engine의 것을 넘긴다.
    pub(crate) fn complete_with(
        &mut self,
        seq: &AtomicU64,
        hub: &Arc<TaskWakerHub>,
        workspace_id: u32,
        task_id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<CompletionReceipt, AgentError> {
        let receipt = self
            .store(seq)
            .complete(workspace_id, task_id, completion, now_ms)?;
        // 같은 보고의 재전송은 이미 알린 종결을 다시 알리지 않는다.
        if !receipt.duplicate {
            self.fire_if_terminal(seq, hub, workspace_id, &receipt.task);
        }
        for t in &receipt.transitioned {
            self.fire_if_terminal(seq, hub, workspace_id, t);
        }
        Ok(receipt)
    }

    /// 입력 결과만 모은다. reducer 실행은 호출자가 단위 밖에서 한다.
    pub fn task_reduce_collect(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let seq = scope.agent_seq().clone();
        let store = self.store(&seq);
        let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
        for tid in inputs {
            let task = match store.get(workspace_id, tid)? {
                Some(t) => t,
                None => return Err(AgentError::TaskNotFound(tid.clone())),
            };
            // 단발 reduce 도 v1 reducer 다. v2 결과는 같은 원칙으로 받지 않는다.
            if task.is_typed() {
                return Err(crate::task::typed_read_refused(
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
    }

    /// 참조·Running 검사를 통과해 삭제된 작업의 handle·실행 결과도 commit 뒤 정리한다.
    pub fn task_delete(
        &mut self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport, AgentError> {
        let seq = scope.agent_seq().clone();
        let settling = crate::runner_thread::stored_handle_ids(&*self.mem, workspace_id);
        let mut store = self.store(&seq);
        crate::task::reject_settling_targets(
            &store,
            workspace_id,
            task_id,
            opts.cascade,
            &settling,
        )?;
        let report = store.delete_checked(workspace_id, task_id, opts)?;
        self.evict_side_keys(workspace_id, report.deleted.clone());
        Ok(report)
    }

    /// 상태나 경과시간 조건 중 하나는 있어야 한다. dry_run과 실제 삭제가 같은 계획을 사용한다.
    pub fn task_purge(
        &mut self,
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
        let keep = crate::runner_thread::stored_handle_ids(&*self.mem, workspace_id);
        let plan = self.store(&seq).plan_sweep(workspace_id, &filter, &keep)?;
        if dry_run {
            return Ok(plan);
        }
        self.store(&seq).apply_sweep_plan(workspace_id, &plan)?;
        self.evict_side_keys(workspace_id, plan.deleted.clone());
        Ok(plan)
    }

    fn evict_side_keys(&mut self, workspace_id: u32, task_ids: Vec<TaskId>) {
        if task_ids.is_empty() {
            return;
        }
        self.effects.0.push(TaskEffect::EvictSideKeys {
            access: self.svc.memory_access().clone(),
            workspace_id,
            task_ids,
        });
    }

    /// handle 이 남은 task id(이전 회차 종료 확인 중이거나 실행 중).
    pub fn stored_handle_ids(&mut self, workspace_id: u32) -> HashSet<TaskId> {
        crate::runner_thread::stored_handle_ids(&*self.mem, workspace_id)
    }

    /// task 한 건의 handle 이 남았는가. 키 하나만 읽는다.
    pub fn has_stored_handle(&mut self, workspace_id: u32, task_id: &TaskId) -> bool {
        crate::runner_host::has_stored_handle(&*self.mem, workspace_id, task_id)
    }

    /// 러너 실행 여부와 저장소의 실제 작업 수.
    pub fn runner_status(&mut self, scope: &TaskScope, workspace_id: u32) -> RunnerStatus {
        let counts = crate::runner_thread::count_ready_running_in(
            &mut *self.mem,
            scope.agent_seq(),
            workspace_id,
        );
        self.svc
            .runner_registry()
            .status_with(scope.waker_hub(), workspace_id, counts)
    }

    /// 외부 완료 신호를 기다리는 작업의 저장된 dispatch handle.
    pub fn dispatch_handle(
        &mut self,
        workspace_id: u32,
        task_id: &str,
    ) -> Option<tasty_agent::DispatchHandle> {
        crate::runner_host::load_dispatch_handle_in(&*self.mem, workspace_id, task_id)
    }

    /// custom 기록 한 줄을 지금 상한으로 저장한다. 세션 토큰으로 부른 agent 세션은 자기 세션에
    /// 지시를 보낸 회차에만 쓴다.
    pub fn report_append(
        &mut self,
        scope: &TaskScope,
        addr: &ReportAddress,
        text: &str,
        writer: Submitter,
    ) -> Result<AppendOutcome, AgentError> {
        let svc = self.svc;
        let bound_here = || crate::report::bound_here(svc, addr, writer);
        crate::runner_host::report_append(
            &mut *self.mem,
            scope.agent_seq(),
            svc.report_limits().get(),
            addr,
            text,
            0,
            bound_here,
        )
        .map(|(outcome, _)| outcome)
    }

    /// DAG 하나의 report. `task` 를 주면 그 task 만, `attempt` 는 그 task 의 회차 하나만 싣는다.
    pub fn dag_report(
        &mut self,
        scope: &TaskScope,
        workspace_ids: &[u32],
        dag_id: &str,
        task: Option<&str>,
        attempt: Option<u32>,
        include_raw: bool,
    ) -> Result<Option<serde_json::Value>, AgentError> {
        let Some((dag, tasks)) = self.dag_get(scope, workspace_ids, dag_id)? else {
            return Ok(None);
        };
        let selected: Vec<_> = match task {
            None => dag
                .task_ids
                .iter()
                .filter_map(|id| tasks.iter().find(|t| &t.id == id))
                .collect(),
            Some(id) => {
                let t = tasks.iter().find(|t| t.id == id).ok_or_else(|| {
                    AgentError::InvalidArgument(format!("task {id} is not in dag {dag_id}"))
                })?;
                vec![t]
            }
        };
        let seq = scope.agent_seq().clone();
        let store = self.store(&seq);
        let entries = selected
            .iter()
            .map(|t| {
                Ok(project_task(
                    t,
                    &store.report_blocks(t)?,
                    &store.sealed_autos(t)?,
                    attempt,
                    include_raw,
                ))
            })
            .collect::<Result<Vec<_>, AgentError>>()?;
        Ok(Some(serde_json::json!({
            "dag": dag.id,
            "workspace_id": dag.workspace_id,
            "name": dag.name,
            "tasks": entries,
        })))
    }
}

impl TaskService {
    /// task 연산 본문을 memory writer 순서 안에서 실행하고 결과를 기다린 뒤 효과를 실행한다.
    /// runner·worker·시험이 부른다. 메인 루프는 [`Self::unit_request`]를 쓴다.
    pub fn transact<R: crate::store_call::FromBlocked + Send + 'static>(
        &self,
        body: impl Fn(&mut TaskTx<'_>) -> R + Send + Sync + 'static,
    ) -> R {
        let svc = self.clone();
        let outcome = self.memory_access().run_blocking(0, move |mem| {
            let mut tx = TaskTx::new(mem, &svc);
            let value = body(&mut tx);
            (value, tx.into_effects())
        });
        match outcome {
            Ok((value, effects)) => {
                effects.apply();
                value
            }
            Err(blocked) => R::from_blocked(blocked),
        }
    }

    /// task 연산 본문을 writer 요청 하나로 만든다. 결과는 `(값, 효과)`이며 받는 쪽이 commit을 확인한
    /// 뒤 효과를 실행한다. 메인 루프가 결과를 기다리지 않고 continuation으로 처리할 때 쓴다.
    pub fn unit_request<T: Send + 'static>(
        &self,
        bytes: usize,
        mut body: impl FnMut(&mut TaskTx<'_>) -> T + Send + 'static,
    ) -> tasty_memory::writer::MemoryRequest {
        let svc = self.clone();
        tasty_memory::writer::MemoryRequest::unit(bytes, move |mem| {
            let mut tx = TaskTx::new(mem, &svc);
            let value = body(&mut tx);
            Ok((value, tx.into_effects()))
        })
    }
}
