//! `TaskStore` — task 의 persistent CRUD + state 전이.
//!
//! 이 파일은 레코드 읽기·쓰기, 생성, 취소를 둔다. 나머지는 책임별 하위 모듈에 있다.
//! 상태 전이와 하류 전파는 `transition`, retry 는 `retry`, 참조 검사를 거치는 삭제와 purge·GC
//! 계획은 `sweep`, 그래프 검증·활성화는 `graph_submit`·`graph_parse`, 결과 보고는 `complete`,
//! 후처리 보고는 `postprocess`, agent 세션 연결은 `agent` 다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use tasty_memory::{ListOpts, MemoryStorage, MemoryValue, PutOpts, Scope};
use tasty_utils::id::WorkspaceId;

use super::contract::{FailureStage, Provenance, TaskContract, TaskFailure, TypedResult};
use super::record_limit;
use super::{
    OnFailure, Readiness, SkipReason, TASK_KEY_PREFIX, TYPED_TASK_KEY_PREFIX,
    TYPED_TASK_RECORD_FORMAT, Task, TaskCommand, TaskGraph, TaskId, TaskResult, TaskState,
    apply_on_failure, contract, route, task_key, typed_task_key,
};
use crate::{AgentError, Result};

pub struct TaskStore<'a> {
    mem: &'a mut dyn MemoryStorage,
    owner: String,
    seq: &'a AtomicU64,
}

/// TaskStore::create의 인자.
pub struct TaskCreateOpts {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub command: TaskCommand,
    pub depends_on: Vec<TaskId>,
    pub on_failure: OnFailure,
    pub metadata: serde_json::Value,
    pub now_ms: u64,
}

impl<'a> TaskStore<'a> {
    /// `owner`는 memory의 owner 필드로 들어간다. 호스트는 보통 `"_host"`를 쓴다.
    pub fn new(
        mem: &'a mut dyn MemoryStorage,
        owner: impl Into<String>,
        seq: &'a AtomicU64,
    ) -> Self {
        Self {
            mem,
            owner: owner.into(),
            seq,
        }
    }

    /// 새 task ID 발급. `t-<now_ms>-<seq:06>`.
    pub fn new_id(&self, now_ms: u64) -> TaskId {
        let s = self.seq.fetch_add(1, Ordering::Relaxed);
        format!("t-{now_ms}-{s:06}")
    }

    /// 이 workspace 에 아직 없는 새 task ID. 순번은 프로세스마다 0 부터라, 재시작 앞뒤로 시계가
    /// 뒤로 가면 같은 ID 가 다시 나올 수 있다. 그때 저장된 task 를 덮어쓰지 않고 다음 순번을 쓴다.
    fn unused_id(&self, now_ms: u64, existing: &[Task]) -> TaskId {
        loop {
            let id = self.new_id(now_ms);
            if !existing.iter().any(|t| t.id == id) {
                return id;
            }
        }
    }

    /// task 영속. 신규/갱신 모두 동일 (overwrite). v2 task 는 별도 namespace 에
    /// envelope 로 감싸 저장한다.
    pub fn put(&mut self, task: &Task) -> Result<()> {
        let scope = Scope::Workspace(task.workspace_id);
        let (key, value) = if task.is_typed() {
            (
                typed_task_key(&task.id)?,
                serde_json::json!({
                    "record_format": TYPED_TASK_RECORD_FORMAT,
                    "task": serde_json::to_value(task)?,
                }),
            )
        } else {
            (task_key(&task.id)?, serde_json::to_value(task)?)
        };
        self.mem.put(
            &self.owner,
            &scope,
            &key,
            &MemoryValue::Json(value),
            &PutOpts::default(),
        )?;
        Ok(())
    }

    /// 단건 조회. v1·v2 namespace 를 모두 본다.
    pub fn get(&self, workspace_id: WorkspaceId, id: &TaskId) -> Result<Option<Task>> {
        let scope = Scope::Workspace(workspace_id);
        if let Some(e) = self.mem.get(&scope, &task_key(id)?)? {
            return decode_v1_record(e.value, id).map(Some);
        }
        match self.mem.get(&scope, &typed_task_key(id)?)? {
            Some(e) => decode_typed_record(e.value, id).map(Some),
            None => Ok(None),
        }
    }

    /// 레코드 revision. task 를 담은 memory 키의 version 이며 쓸 때마다 커진다. 사건을 받은 쪽이
    /// 다시 읽은 레코드가 그 사건 이후의 것인지 비교할 때 쓴다. 레코드가 없으면 `None`.
    pub fn revision(&self, workspace_id: WorkspaceId, id: &TaskId) -> Result<Option<u64>> {
        let scope = Scope::Workspace(workspace_id);
        if let Some(e) = self.mem.get(&scope, &task_key(id)?)? {
            return Ok(Some(e.version));
        }
        Ok(self
            .mem
            .get(&scope, &typed_task_key(id)?)?
            .map(|e| e.version))
    }

    /// 워크스페이스 전체 task 목록(v1 뒤에 v2).
    pub fn list(&self, workspace_id: WorkspaceId) -> Result<Vec<Task>> {
        let scope = Scope::Workspace(workspace_id);
        let list_prefix = |prefix: &str| {
            self.mem.list(
                &scope,
                &ListOpts {
                    prefix: Some(prefix.to_string()),
                    ..Default::default()
                },
            )
        };
        let v1 = list_prefix(TASK_KEY_PREFIX)?;
        let v2 = list_prefix(TYPED_TASK_KEY_PREFIX)?;
        let mut out = Vec::with_capacity(v1.len() + v2.len());
        for e in v1 {
            if matches!(e.value, MemoryValue::Json(_)) {
                out.push(decode_v1_record(e.value, &e.key)?);
            }
        }
        for e in v2 {
            out.push(decode_typed_record(e.value, &e.key)?);
        }
        Ok(out)
    }

    /// task 삭제 (드물게 사용; 보통은 Cancelled 상태로 유지).
    pub fn delete(&mut self, workspace_id: WorkspaceId, id: &TaskId) -> Result<()> {
        let scope = Scope::Workspace(workspace_id);
        let key = if self.mem.get(&scope, &typed_task_key(id)?)?.is_some() {
            if let Some(task) = self.get(workspace_id, id)? {
                self.delete_report_blocks(&task)?;
            }
            typed_task_key(id)?
        } else {
            task_key(id)?
        };
        self.mem.delete(&self.owner, &scope, &key, None)?;
        Ok(())
    }

    /// 신규 task 생성. 사이클 검출 + 초기 state 계산 후 영속.
    /// `now_ms`는 호스트가 주입 (테스트 결정성).
    pub fn create(&mut self, opts: TaskCreateOpts) -> Result<Task> {
        self.create_with_contract(opts, None)
    }

    /// v2 계약을 가진 task 를 만든다. 계약 검사가 실패하면 아무것도 저장하지 않는다.
    pub fn create_typed(&mut self, opts: TaskCreateOpts, contract: TaskContract) -> Result<Task> {
        self.create_with_contract(opts, Some(contract))
    }

    fn create_with_contract(
        &mut self,
        opts: TaskCreateOpts,
        contract: Option<TaskContract>,
    ) -> Result<Task> {
        let TaskCreateOpts {
            workspace_id,
            name,
            command,
            depends_on,
            on_failure,
            metadata,
            now_ms,
        } = opts;
        if contract.is_none() && matches!(command, TaskCommand::Agent { .. }) {
            return Err(AgentError::InvalidArgument(
                super::agent::AGENT_NEEDS_CONTRACT.into(),
            ));
        }
        // 입력에서 이름을 받는 wait_barrier 는 v2 계약의 input_mapping 이 있어야 한다.
        if contract.is_none() && matches!(command, TaskCommand::WaitBarrier { name: None }) {
            return Err(AgentError::InvalidArgument(
                "wait_barrier needs a barrier name (name)".into(),
            ));
        }
        let mut existing = self.list(workspace_id)?;
        let id = self.unused_id(now_ms, &existing);

        if let OnFailure::Fallback {
            task: fb_task,
            inline,
        } = &on_failure
        {
            match (fb_task.is_some(), inline.is_some()) {
                (false, false) => {
                    return Err(AgentError::InvalidArgument(
                        "OnFailure::Fallback requires either 'task' or 'inline'".into(),
                    ));
                }
                (true, true) => {
                    return Err(AgentError::InvalidArgument(
                        "OnFailure::Fallback cannot have both 'task' and 'inline'".into(),
                    ));
                }
                _ => {}
            }
        }

        let known: HashSet<&TaskId> = existing.iter().map(|t| &t.id).collect();
        for dep in &depends_on {
            if !known.contains(dep) {
                return Err(AgentError::UnknownDependency(dep.clone()));
            }
        }

        // 없는 fallback을 저장하면 main 실패 뒤 downstream이 계속 Waiting에 남는다.
        // inline fallback은 실패 시 생성하므로 여기서 존재 여부를 검사하지 않는다.
        if let OnFailure::Fallback {
            task: Some(fb_id), ..
        } = &on_failure
            && !known.contains(fb_id)
        {
            return Err(AgentError::UnknownDependency(fb_id.clone()));
        }

        // Reduce 입력은 생성 시 존재해야 한다. 사이클은 TaskGraph가 검사한다.
        if let TaskCommand::Reduce { inputs, .. } = &command {
            for input_id in inputs {
                if !known.contains(input_id) {
                    return Err(AgentError::UnknownDependency(input_id.clone()));
                }
            }
        }

        let mut new_task = Task {
            id: id.clone(),
            workspace_id,
            name,
            command,
            depends_on,
            state: TaskState::Waiting,
            created_at: now_ms,
            started_at: None,
            finished_at: None,
            result: None,
            on_failure,
            metadata,
            reserved_for_fallback: false,
            contract,
            typed_result: None,
            graph_id: None,
            input_snapshot: None,
            accepted: None,
            report_token: None,
            attempt: None,
            route: None,
            skip: None,
        };
        contract::check_task(&new_task, "", |id| existing.iter().find(|t| &t.id == id))
            .map_err(|f| AgentError::TypeContract(Box::new(f)))?;
        // 전이 대상은 함께 제출하는 task 라 그래프 제출에서만 정한다.
        if new_task
            .contract
            .as_ref()
            .is_some_and(|c| c.transitions.is_some())
        {
            return Err(AgentError::InvalidArgument(
                "transitions select tasks submitted together; use a task graph submission".into(),
            ));
        }

        let inactive = self.inactive_task_ids(workspace_id, &existing)?;
        existing.push(new_task.clone());
        {
            let graph = TaskGraph::build(&existing).with_inactive(inactive);
            graph.detect_cycles()?;
            if let Some(state) = graph.evaluate_readiness(&new_task.id) {
                new_task.state = state;
            }
        }

        // 저장할 그대로(초기 상태 포함) 잰다.
        record_limit::check(&new_task, "the definition").map_err(AgentError::InvalidArgument)?;
        self.put(&new_task)?;

        // 먼저 생성된 fallback이 아직 Ready라면 Waiting으로 되돌린다.
        // 두 create 사이에 이미 실행됐다면 되돌릴 수 없으므로 호출자는
        // create_reserved_for_fallback으로 미리 예약해야 한다.
        // main이 생겼으므로 예약을 해제하고 일반 fallback 대기 규칙을 적용한다.
        if let OnFailure::Fallback {
            task: Some(fb_id), ..
        } = &new_task.on_failure
            && let Some(mut fb) = self.get(workspace_id, fb_id)?
        {
            let mut changed = false;
            if fb.reserved_for_fallback {
                fb.reserved_for_fallback = false;
                changed = true;
            }
            if matches!(fb.state, TaskState::Ready) {
                fb.state = TaskState::Waiting;
                changed = true;
            }
            if changed {
                self.put(&fb)?;
            }
        }

        Ok(new_task)
    }

    /// main 생성 전부터 Waiting으로 예약해 두 create 사이의 조기 실행을 막는다.
    /// 참조할 main을 끝내 만들지 않으면 계속 대기하므로 호출자가 삭제해야 한다.
    pub fn create_reserved_for_fallback(&mut self, opts: TaskCreateOpts) -> Result<Task> {
        let mut task = self.create(opts)?;
        if !matches!(task.state, TaskState::Waiting) {
            task.state = TaskState::Waiting;
        }
        task.reserved_for_fallback = true;
        self.put(&task)?;
        Ok(task)
    }

    /// 사용자가 명시적으로 cancel.
    pub fn cancel(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>)> {
        let task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if task.state.is_terminal() {
            return Err(AgentError::AlreadyTerminal(task.state.name().to_string()));
        }
        self.set_state(workspace_id, id, TaskState::Cancelled, now_ms)
    }
}

impl TaskStore<'_> {
    /// 활성화 레코드가 없는 그래프의 task id.
    pub(super) fn inactive_task_ids(
        &self,
        workspace_id: WorkspaceId,
        tasks: &[Task],
    ) -> Result<HashSet<TaskId>> {
        let scope = Scope::Workspace(workspace_id);
        let mut active: std::collections::HashMap<&str, bool> = Default::default();
        let mut out = HashSet::new();
        for t in tasks {
            let Some(gid) = t.graph_id.as_deref() else {
                continue;
            };
            let is_active = match active.get(gid) {
                Some(a) => *a,
                None => {
                    let a = self.mem.get(&scope, &graph_key(gid)?)?.is_some();
                    active.insert(gid, a);
                    a
                }
            };
            if !is_active {
                out.insert(t.id.clone());
            }
        }
        Ok(out)
    }

    /// readiness 평가용 그래프. 활성화되지 않은 그래프의 task 는 대기로 둔다.
    pub(super) fn readiness_graph<'t>(
        &self,
        workspace_id: WorkspaceId,
        tasks: &'t [Task],
    ) -> Result<TaskGraph<'t>> {
        Ok(TaskGraph::build(tasks).with_inactive(self.inactive_task_ids(workspace_id, tasks)?))
    }

    /// 성공으로 끝나는 v2 task 의 경로를 고른다. 고르지 못하면 `route` 단계 실패로 바꾼다.
    /// 결과와 같은 레코드에 기록하므로 "결과는 있는데 경로는 미확정" 인 상태가 저장되지 않는다.
    pub(super) fn settle_route(&self, task: &mut Task, state: TaskState) -> TaskState {
        if state != TaskState::Succeeded || !task.is_typed() {
            return state;
        }
        let ws = task.workspace_id;
        match route::decide_route(task, &|id| self.get(ws, id).ok().flatten()) {
            Ok(decision) => {
                task.route = decision;
                state
            }
            Err(failure) => {
                let error = failure.message.clone();
                if let Some(typed) = task.typed_result.as_mut() {
                    typed.error = Some(failure);
                }
                task.result = task.typed_result.as_ref().map(contract::project_v1);
                TaskState::Failed { error }
            }
        }
    }

    /// 실행 직전에 해석한 입력을 기록한다. 원본 계약과 command 는 바꾸지 않는다.
    pub fn set_input_snapshot(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        snapshot: super::binding::InputSnapshot,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !task.is_typed() {
            return Err(AgentError::InvalidArgument(format!(
                "task {id} is not a typed task; it has no input snapshot"
            )));
        }
        task.input_snapshot = Some(snapshot);
        // 해석한 입력이 레코드를 상한 너머로 키우면 실행하지 않고 입력 단계 실패로 남긴다.
        // 그대로 두면 결과를 줄여도 레코드를 저장할 수 없다.
        if let Err(message) = record_limit::check(&task, "the resolved input")
            && let Some(snap) = task.input_snapshot.as_mut()
        {
            snap.value = super::types::TypedValue::Null;
            snap.execution = Default::default();
            snap.failure = Some(TaskFailure {
                location: Some("/bindings".into()),
                ..TaskFailure::new(FailureStage::Input, message)
            });
        }
        self.put(&task)?;
        Ok(task)
    }

    /// custom 비동기 task 가 dispatch 때 받은 접수 응답을 저장한다. 결과를 확정할 때
    /// `raw.accepted` 로 싣는다. `retry` 가 지운다.
    pub fn set_accepted(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        accepted: super::contract::AcceptedResponse,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !task.is_typed() {
            return Err(AgentError::InvalidArgument(format!(
                "task {id} is not a typed task; it has no raw result"
            )));
        }
        task.accepted = Some(accepted);
        self.put(&task)?;
        Ok(task)
    }
}

#[cfg(test)]
impl TaskStore<'_> {
    pub(super) fn memory(&self) -> &dyn MemoryStorage {
        &*self.mem
    }
}

pub(super) fn graph_key(graph_id: &str) -> Result<String> {
    crate::component_key(super::TASK_GRAPH_KEY_PREFIX, "graph id", graph_id)
}

mod agent;
mod complete;
#[cfg(test)]
pub(super) use complete::FAIL_COMPLETION_PUT;
mod graph_parse;
mod graph_submit;
pub use graph_submit::*;
mod postprocess;
mod report;
mod retry;
mod sweep;
pub use sweep::{TaskDeleteOpts, TaskDeleteReport, TaskPurgeFilter, TaskSweepPlan};
mod transition;

fn decode_v1_record(value: MemoryValue, label: &str) -> Result<Task> {
    let MemoryValue::Json(v) = value else {
        return Err(AgentError::InvalidArgument(format!(
            "task entry is not json: {label}"
        )));
    };
    let task: Task = serde_json::from_value(v)?;
    if task.is_typed() {
        // v2 계약을 가진 레코드는 v2 namespace 에만 저장한다. v1 자리에 있으면 v1 앱이
        // 계약을 무시하고 실행할 수 있으므로 손상으로 본다.
        return Err(AgentError::InvalidArgument(format!(
            "task entry {label} carries a v2 contract in the v1 namespace"
        )));
    }
    Ok(task)
}

fn decode_typed_record(value: MemoryValue, label: &str) -> Result<Task> {
    let MemoryValue::Json(mut v) = value else {
        return Err(AgentError::InvalidArgument(format!(
            "typed task entry is not json: {label}"
        )));
    };
    let format = v.get("record_format").and_then(|f| f.as_str());
    if format != Some(TYPED_TASK_RECORD_FORMAT) {
        return Err(AgentError::InvalidArgument(format!(
            "typed task entry {label} has unsupported record_format {format:?} \
             (supported: {TYPED_TASK_RECORD_FORMAT})"
        )));
    }
    let task: Task = serde_json::from_value(v["task"].take())?;
    if !task.is_typed() {
        return Err(AgentError::InvalidArgument(format!(
            "typed task entry {label} has no contract"
        )));
    }
    Ok(task)
}

/// Waiting task 의 다음 상태와(Skipped 면) 그 이유. 선행 실패에는 실패 정책을 적용하고,
/// 경로가 선택되지 않은 것에는 적용하지 않는다(fallback·continue_downstream 으로 되살리지 않는다).
fn settle_target(
    graph: &TaskGraph<'_>,
    task: &Task,
    all: &[Task],
) -> Option<(TaskState, Option<SkipReason>)> {
    match graph.readiness(&task.id)? {
        Readiness::Ready => Some((TaskState::Ready, None)),
        Readiness::NotSelected => Some((TaskState::Skipped, Some(SkipReason::BranchNotSelected))),
        Readiness::Unavailable(reason) => {
            let next = apply_on_failure(task, all)?;
            let skip = (next == TaskState::Skipped).then_some(reason).flatten();
            Some((next, skip))
        }
    }
}

/// v2 task 의 종결 전이를 결과와 맞춘다. 확정된 유효 출력이 없으면 성공으로 끝내지
/// 않고 실패로 바꾼다. 실패로 끝나는데 결과가 없으면 실패 사유를 결과로 남긴다.
/// 보고된 결과를 task 에 기록한다. v2 는 계약으로 확정하고 v1 형식 필드에는 그 투영을 둔다.
fn record_result(task: &mut Task, result: TaskResult) {
    let Some(c) = &task.contract else {
        task.result = Some(result);
        return;
    };
    let mut typed = contract::finalize_result(task, c, &result);
    // 입력 해석에 실패해 실행하지 않았다면 실패 단계는 input 이다.
    if result.error.is_some()
        && let Some(failure) = task.input_snapshot.as_ref().and_then(|s| s.failure.clone())
    {
        typed.error = Some(failure);
    }
    task.result = Some(contract::project_v1(&typed));
    task.typed_result = Some(typed);
    // 접수 응답은 결과의 raw 로 옮겼다. 같은 값을 두 곳에 두지 않는다.
    task.accepted = None;
}

fn settle_typed_terminal(task: &mut Task, requested: TaskState) -> TaskState {
    if !task.is_typed() {
        return requested;
    }
    // 실패 결과로 바꿀 때도 접수 응답은 한 벌만 결과에 남긴다. 확정 전이면 task 에서 옮기고,
    // 이미 확정한 결과를 바꾸는 경우면 그 결과의 것을 이어받는다.
    let failure_result = |task: &mut Task, failure: TaskFailure| TypedResult {
        has_output: false,
        output: super::types::TypedValue::Null,
        raw: contract::RawResult {
            accepted: task.accepted.take().or_else(|| {
                task.typed_result
                    .as_ref()
                    .and_then(|t| t.raw.accepted.clone())
            }),
            ..Default::default()
        },
        error: Some(failure),
        provenance: Provenance {
            contract_version: task
                .contract
                .as_ref()
                .map(|c| c.contract_version)
                .unwrap_or_default(),
            kind: contract::command_kind(&task.command).to_string(),
            output_source: "none".to_string(),
        },
    };
    match requested {
        TaskState::Succeeded => {
            let failure = match &task.typed_result {
                Some(t) if t.has_output && t.error.is_none() => return TaskState::Succeeded,
                Some(t) => t.error.clone().unwrap_or_else(|| {
                    TaskFailure::new(FailureStage::OutputValidation, "no output was produced")
                }),
                None => {
                    let f = TaskFailure::new(
                        FailureStage::Persistence,
                        "no result was recorded before completion",
                    );
                    let typed = failure_result(task, f.clone());
                    task.result = Some(contract::project_v1(&typed));
                    task.typed_result = Some(typed);
                    f
                }
            };
            TaskState::Failed {
                error: failure.message,
            }
        }
        TaskState::Failed { error } => {
            if task.typed_result.as_ref().is_none_or(|t| t.error.is_none()) {
                let typed = failure_result(
                    task,
                    TaskFailure::new(FailureStage::Execution, error.clone()),
                );
                task.result = Some(contract::project_v1(&typed));
                task.typed_result = Some(typed);
            }
            TaskState::Failed { error }
        }
        other => other,
    }
}
