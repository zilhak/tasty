//! `TaskStore` — task 의 persistent CRUD + state 전이.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use tasty_memory::{ListOpts, MemoryStorage, MemoryValue, PutOpts, Scope};
use tasty_utils::id::WorkspaceId;

use super::contract::{FailureStage, Provenance, TaskContract, TaskFailure, TypedResult};
use super::{
    InlineFallbackSpec, OnFailure, Readiness, SkipReason, TASK_KEY_PREFIX, TYPED_TASK_KEY_PREFIX,
    TYPED_TASK_RECORD_FORMAT, Task, TaskCommand, TaskGraph, TaskId, TaskResult, TaskState,
    apply_on_failure, contract, is_valid_transition, referencing_task_ids, route, task_key,
    transitive_referencing_task_ids, typed_task_key,
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

/// [`TaskStore::delete_checked`] 옵션.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskDeleteOpts {
    /// 대상의 전이적 참조자도 함께 삭제한다.
    pub cascade: bool,
    /// 참조 검사만 생략한다. Running 작업은 삭제할 수 없다.
    pub force: bool,
}

/// [`TaskStore::delete_checked`] 결과. `cascade` 가 아니면 `deleted` 는 항상
/// 대상 task id 하나뿐이다.
#[derive(Debug, Clone, Default)]
pub struct TaskDeleteReport {
    pub deleted: Vec<TaskId>,
}

/// [`TaskStore::plan_sweep`] 필터 — 상태 집합 ∩ 경과시간 조건을 모두 만족하는
/// task 만 후보로 삼는다. 둘 다 `None` 이면 워크스페이스 전체가 후보가 되므로,
/// 최소 하나를 지정하도록 강제하는 건 호출자(`Core::task_purge`/CLI) 의 몫이다.
#[derive(Debug, Clone)]
pub struct TaskPurgeFilter {
    /// TaskState::name()과 비교할 상태 이름. None이면 상태를 제한하지 않는다.
    pub states: Option<Vec<String>>,
    /// `now_ms - 기준시각 >= older_than_ms` 인 task 만 후보. 기준시각은 terminal
    /// task 는 `finished_at`, 그 외(Waiting/Ready)는 `created_at`.
    pub older_than_ms: Option<u64>,
    pub now_ms: u64,
}

/// 조회와 실제 삭제가 함께 사용하는 정리 계획.
#[derive(Debug, Clone, Default)]
pub struct TaskSweepPlan {
    /// 필터를 만족하고, Running 이 아니며, 후보 집합 밖에서 참조되지 않아
    /// 안전하게 지울 수 있는 task id.
    pub deleted: Vec<TaskId>,
    /// 필터는 만족했지만 후보 집합 밖의 task 가 여전히 참조 중이라 이번 sweep
    /// 에서는 제외된 task id — 그 참조자가 먼저(또는 같이) 지워지면 이후 sweep
    /// 에서 지워진다.
    pub retained: Vec<TaskId>,
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

    /// task의 state를 변경. 변경 규칙은 [`TaskState`] 문서 참조.
    /// 변경 후 downstream의 readiness를 자동 재평가해 `Waiting → Ready/Skipped`로
    /// 전이시키고 영속한다. 반환값은 (갱신된 자기 자신, 자동 전이된 downstream).
    pub fn set_state(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>)> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !is_valid_transition(&task.state, &new_state) {
            return Err(AgentError::InvalidTransition {
                from: task.state.name().to_string(),
                to: new_state.name().to_string(),
            });
        }
        let new_state = settle_typed_terminal(&mut task, new_state);
        let new_state = self.settle_route(&mut task, new_state);
        match new_state {
            TaskState::Running => {
                task.started_at = Some(now_ms);
                task.route = None;
                task.skip = None;
                if let Some(attempt) = super::attempt::next_attempt(&task, now_ms) {
                    task.attempt = Some(attempt);
                }
            }
            TaskState::Succeeded
            | TaskState::Failed { .. }
            | TaskState::Cancelled
            | TaskState::Skipped => {
                task.finished_at = Some(now_ms);
                super::postprocess::close_phase(&mut task);
            }
            _ => {}
        }
        task.state = new_state.clone();
        self.put(&task)?;
        let transitioned = self.propagate_transition(workspace_id, &task, now_ms)?;
        Ok((task, transitioned))
    }

    /// 저장을 마친 상태 전이의 후속 효과: fallback 승격·정리, 하류 readiness, 이 task 를
    /// fallback 으로 둔 main 의 하류 재평가. 같은 전이로 다시 불러도 결과가 같다(이미 옮긴
    /// task 는 Waiting 이 아니라 건너뛴다).
    fn propagate_transition(
        &mut self,
        workspace_id: WorkspaceId,
        task: &Task,
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let id = &task.id;
        let new_state = task.state.clone();
        let mut transitioned = Vec::new();

        if matches!(new_state, TaskState::Failed { .. })
            && let OnFailure::Fallback {
                task: fb_id_opt,
                inline: inline_opt,
            } = task.on_failure.clone()
        {
            if let Some(fb_id) = fb_id_opt
                && let Some(fb) = self.advance_existing_fallback(workspace_id, &task.id, &fb_id)?
            {
                transitioned.push(fb);
            }
            if let Some(spec) = inline_opt
                && let Some(new_fb) =
                    self.materialize_inline_fallback(workspace_id, task, *spec, now_ms)?
            {
                transitioned.push(new_fb);
            }
        }

        // main이 실패 없이 끝났다면 실행할 일이 없는 기존 fallback도 종결한다.
        // inline fallback은 실패 시에만 생성되므로 정리할 대상이 없다.
        // v2 fallback 은 하류 판정(cascade)이 이유와 함께 마감한다.
        if !task.is_typed()
            && matches!(
                new_state,
                TaskState::Succeeded | TaskState::Cancelled | TaskState::Skipped
            )
            && let OnFailure::Fallback {
                task: Some(fb_id), ..
            } = task.on_failure.clone()
            && matches!(
                self.get(workspace_id, &fb_id)?.map(|t| t.state),
                Some(TaskState::Waiting)
            )
        {
            let (fb_task, more) =
                self.set_state(workspace_id, &fb_id, TaskState::Skipped, now_ms)?;
            transitioned.push(fb_task);
            transitioned.extend(more);
        }

        transitioned.extend(self.cascade_downstream(workspace_id, id, now_ms)?);

        // terminal 전이 시: 자기를 fallback 으로 지정한 main task 가 있으면 그 main
        // 의 downstream 도 재평가 (main 입장에선 fallback 결과로 effective state 가 정해짐).
        if matches!(
            new_state,
            TaskState::Succeeded
                | TaskState::Failed { .. }
                | TaskState::Cancelled
                | TaskState::Skipped
        ) {
            let all_now = self.list(workspace_id)?;
            let parent_main_ids: Vec<TaskId> = all_now
                .iter()
                .filter(|t| match &t.on_failure {
                    // existing 경로: main 의 task field 가 id 를 직접 가리킴.
                    OnFailure::Fallback {
                        task: Some(fb_id), ..
                    } => fb_id == id,
                    _ => false,
                })
                .map(|t| t.id.clone())
                .collect();
            // inline 경로: self.metadata.fallback_of 가 main id — main 을 찾으려면 reverse lookup.
            let mut inline_main_ids: Vec<TaskId> = Vec::new();
            if let Some(self_task) = all_now.iter().find(|t| t.id == *id)
                && let Some(main_id) = self_task
                    .metadata
                    .get("fallback_of")
                    .and_then(|v| v.as_str())
            {
                inline_main_ids.push(main_id.to_string());
            }
            for main_id in parent_main_ids.into_iter().chain(inline_main_ids) {
                transitioned.extend(self.cascade_downstream(workspace_id, &main_id, now_ms)?);
            }
        }
        Ok(transitioned)
    }

    /// `set_state`의 Failed→Fallback 분기 중 "케이스 1: existing fallback" 처리.
    /// fallback 대상이 `Ready`/`Skipped` 로 진행 가능하면 그 상태로 올리고 반환.
    fn advance_existing_fallback(
        &mut self,
        workspace_id: WorkspaceId,
        main_task_id: &TaskId,
        fb_id: &TaskId,
    ) -> Result<Option<Task>> {
        let Some(mut fb) = self.get(workspace_id, fb_id)? else {
            // 옛 레코드의 끊긴 참조는 남을 수 있다. downstream 대기 원인을 로그에 남긴다.
            tracing::warn!(
                task_id = %main_task_id,
                fallback_task_id = %fb_id,
                "on_failure.fallback.task references a task that no longer exists; \
                 downstream depending on this task will remain Waiting indefinitely"
            );
            return Ok(None);
        };
        let all_now = self.list(workspace_id)?;
        let Some(target) = self
            .readiness_graph(workspace_id, &all_now)?
            .evaluate_readiness(fb_id)
        else {
            return Ok(None);
        };
        if target == TaskState::Waiting || !is_valid_transition(&fb.state, &target) {
            return Ok(None);
        }
        fb.state = target;
        self.put(&fb)?;
        Ok(Some(fb))
    }

    /// `set_state`의 Failed→Fallback 분기 중 "케이스 2: inline → 동적 생성" 처리.
    /// 같은 main 이 이미 inline fallback 을 만든 적 있으면(idempotency) `None`.
    fn materialize_inline_fallback(
        &mut self,
        workspace_id: WorkspaceId,
        main_task: &Task,
        spec: InlineFallbackSpec,
        now_ms: u64,
    ) -> Result<Option<Task>> {
        let existing = self.list(workspace_id)?;
        let already = existing.iter().any(|t| {
            t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(main_task.id.as_str())
        });
        if already {
            return Ok(None);
        }
        let mut metadata = spec.metadata;
        if !metadata.is_object() {
            metadata = serde_json::json!({});
        }
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "fallback_of".into(),
                serde_json::Value::String(main_task.id.clone()),
            );
        }
        let opts = TaskCreateOpts {
            workspace_id,
            name: spec.name,
            command: spec.command,
            depends_on: spec
                .depends_on_override
                .unwrap_or_else(|| main_task.depends_on.clone()),
            on_failure: spec.on_failure,
            metadata,
            now_ms,
        };
        Ok(Some(self.create(opts)?))
    }

    /// task의 result를 기록 (state 전이는 별도). 보통 set_state(Succeeded/Failed) 전에 호출.
    pub fn set_result(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        result: TaskResult,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        record_result(&mut task, result);
        self.put(&task)?;
        Ok(task)
    }

    /// `task_id`의 모든 transitive downstream에서 `Waiting` 상태인 것들을 평가해
    /// 가능하면 `Ready/Skipped`로 전이. on_failure 정책도 함께 적용.
    ///
    /// `now_ms`는 `Skipped`(terminal)로 전이하는 downstream의 `finished_at`을 채우는 데
    /// 쓰인다 — 이 경로는 `set_state`를 거치지 않고 상태를 직접 갈아끼우므로, `set_state`의
    /// terminal 타임스탬프 기록을 스스로 재현해야 한다.
    fn cascade_downstream(
        &mut self,
        workspace_id: WorkspaceId,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let mut all = self.list(workspace_id)?;
        let downstream_ids = TaskGraph::build(&all).transitive_downstream(task_id);
        self.settle_waiting(workspace_id, &mut all, &downstream_ids, now_ms)
    }

    /// `ids` 순서대로 Waiting task 의 readiness 를 평가하고 바뀐 task 를 저장한다.
    ///
    /// 저장소 목록은 호출자가 한 번 읽어 `all` 로 넘기고, 이 함수는 저장한 상태를 `all` 에도
    /// 반영해 다음 평가에 쓴다. 저장소를 task 마다 다시 읽지 않는다. 호출 동안 다른 쓰기가
    /// 끼지 않는 것은 `&mut self` 가 보장한다. 활성화 여부는 호출 시점에 한 번 읽는다.
    /// `ids` 가 위상 순서가 아니어도 되도록 바뀐 것이 없을 때까지 다시 훑는다. 합류 task 가
    /// 아직 Waiting 인 선행보다 먼저 나와도 그 선행이 끝난 뒤 다시 평가된다.
    pub(super) fn settle_waiting(
        &mut self,
        workspace_id: WorkspaceId,
        all: &mut [Task],
        ids: &[TaskId],
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let inactive = self.inactive_task_ids(workspace_id, all)?;
        let index: std::collections::HashMap<TaskId, usize> = all
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        let mut updated = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            for id in ids {
                let Some(&i) = index.get(id) else {
                    continue;
                };
                if !matches!(all[i].state, TaskState::Waiting) {
                    continue;
                }
                let target = {
                    let graph = TaskGraph::build(all).with_inactive(inactive.clone());
                    settle_target(&graph, &all[i], all)
                };
                if let Some((next, skip)) = target
                    && next != TaskState::Waiting
                    && is_valid_transition(&all[i].state, &next)
                {
                    let mut nt = all[i].clone();
                    if next.is_terminal() {
                        nt.finished_at = Some(now_ms);
                    }
                    nt.state = next;
                    nt.skip = skip;
                    #[cfg(test)]
                    if graph_submit::FAIL_ACTIVATION_PUT.with(|f| f.replace(false)) {
                        return Err(AgentError::InvalidArgument(
                            "injected activation failure".into(),
                        ));
                    }
                    self.put(&nt)?;
                    all[i] = nt.clone();
                    updated.push(nt);
                    changed = true;
                }
            }
        }
        Ok(updated)
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

    /// retry. 현재 state가 Failed/Cancelled/Skipped/Unknown인 경우만 허용.
    /// `reset_downstream=true`면 downstream 중 Skipped/Failed인 것도 Waiting으로 되돌림.
    pub fn retry(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        match &task.state {
            TaskState::Failed { .. }
            | TaskState::Cancelled
            | TaskState::Skipped
            | TaskState::Unknown => {}
            other => {
                return Err(AgentError::InvalidTransition {
                    from: other.name().to_string(),
                    to: "waiting (retry)".to_string(),
                });
            }
        }
        self.refuse_retry_after_fallback(workspace_id, &task)?;
        self.refuse_retry_of_unselected(workspace_id, &task)?;
        if reset_downstream && task.is_typed() {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {id} cannot be retried with reset_downstream: downstream tasks already \
                 settled on this run's failure or route and are not rewound; retry without it, \
                 or submit a new graph for the follow-up work"
            )));
        }
        task.state = TaskState::Waiting;
        task.started_at = None;
        task.finished_at = None;
        task.result = None;
        task.typed_result = None;
        task.input_snapshot = None;
        task.route = None;
        task.skip = None;
        self.put(&task)?;

        // readiness 즉시 평가 — deps 가 이미 종결(예: 여전히 실패/skip 상태)이면 이 자리에서
        // 곧장 Skipped 로 되돌아갈 수 있다. set_state 를 거치지 않는 직접-put 이므로, 그
        // terminal 타임스탬프 기록을 여기서 재현한다(cascade_downstream 과 동일 이유).
        let all = self.list(workspace_id)?;
        if let Some((next, skip)) =
            settle_target(&self.readiness_graph(workspace_id, &all)?, &task, &all)
            && next != TaskState::Waiting
        {
            let is_terminal = next.is_terminal();
            let mut nt = task.clone();
            nt.state = next;
            nt.skip = skip;
            if is_terminal {
                nt.finished_at = Some(now_ms);
            }
            self.put(&nt)?;
            task = nt;
        }

        if reset_downstream {
            let downstream = TaskGraph::build(&all).transitive_downstream(id);
            for d_id in downstream {
                if let Some(mut d) = self.get(workspace_id, &d_id)?
                    && matches!(
                        d.state,
                        TaskState::Skipped | TaskState::Failed { .. } | TaskState::Cancelled
                    )
                {
                    d.state = TaskState::Waiting;
                    d.started_at = None;
                    d.finished_at = None;
                    d.result = None;
                    d.typed_result = None;
                    d.input_snapshot = None;
                    d.route = None;
                    d.skip = None;
                    self.put(&d)?;
                }
            }
            // downstream 모두 갱신했으니 cascade 한번 더 — 재-skip 되는 downstream 의
            // finished_at 은 cascade_downstream 이 now_ms 로 다시 채운다.
            self.cascade_downstream(workspace_id, id, now_ms)?;
        }

        Ok(task)
    }

    /// v2 task 의 fallback 이 이미 실행됐으면 재시도를 거절한다. main 이 다시 성공하면 둘 중
    /// 하나를 받는 소비자(`one_of`)가 성공한 원본 둘을 보게 되고, 이미 끝난 fallback 의 결과를
    /// 되돌릴 수도 없다. fallback 이 실패했거나 실행 전에 끝났으면 재시도할 수 있다.
    fn refuse_retry_after_fallback(&self, workspace_id: WorkspaceId, task: &Task) -> Result<()> {
        let OnFailure::Fallback {
            task: Some(fallback_id),
            ..
        } = &task.on_failure
        else {
            return Ok(());
        };
        if !task.is_typed() {
            return Ok(());
        }
        let Some(fallback) = self.get(workspace_id, fallback_id)? else {
            return Ok(());
        };
        if matches!(
            fallback.state,
            TaskState::Ready | TaskState::Running | TaskState::Succeeded
        ) {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {} cannot be retried: its fallback {fallback_id} is {} and \
                 stays the outcome of this run; submit a new task to run it again",
                task.id,
                fallback.state.name()
            )));
        }
        Ok(())
    }

    /// 경로가 선택되지 않아 건너뛴 v2 task 를 다시 판정해도 선택되지 않으면 재시도를 거절한다.
    /// 받아들이면 같은 판정으로 곧장 건너뛰어 아무것도 실행하지 않은 채 성공 응답만 남는다.
    fn refuse_retry_of_unselected(&self, workspace_id: WorkspaceId, task: &Task) -> Result<()> {
        if !task.is_typed() || !route::is_not_selected(task) {
            return Ok(());
        }
        let all = self.list(workspace_id)?;
        if matches!(
            self.readiness_graph(workspace_id, &all)?
                .readiness(&task.id),
            Some(Readiness::NotSelected)
        ) {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {} cannot be retried: its branch was not selected and is still not \
                 selected, so it would be skipped again without running; submit a new task to \
                 run it",
                task.id
            )));
        }
        Ok(())
    }

    /// 참조 무결성 + 상태 제약을 지키는 task 삭제. `raw delete`
    /// (위 [`Self::delete`])는 이 검사들을 전혀 하지 않으므로 직접 호출하면
    /// dangling 참조·영구 `Waiting`·자원 누수를 만들 수 있다 — 호스트/CLI 는
    /// 항상 이 메서드를 거쳐야 한다.
    ///
    /// - `Running` 상태는 `cascade`/`force` 와 무관하게 항상 거부 —
    ///   먼저 `cancel` 로 정리해야 한다.
    /// - 기본(둘 다 `false`): 참조자가 하나라도 있으면 거부하고 그 목록을 반환.
    /// - `cascade`: 전이적 참조자 전부를 함께 지운다. 참조자 중 `Running` 이
    ///   있으면 그 하나 때문에 통째로 거부한다(부분 cascade 없음).
    /// - `force`(비-cascade): 참조 검사만 생략. 대상 자체의 `Running` 제약은
    ///   여전히 적용된다.
    pub fn delete_checked(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport> {
        let task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if matches!(task.state, TaskState::Running) {
            return Err(AgentError::TaskRunning(id.clone()));
        }

        let all = self.list(workspace_id)?;
        let targets: Vec<TaskId> = if opts.cascade {
            let mut seen: HashSet<TaskId> = HashSet::new();
            seen.insert(id.clone());
            seen.extend(transitive_referencing_task_ids(&all, id));
            for t_id in &seen {
                if let Some(t) = all.iter().find(|t| &t.id == t_id)
                    && matches!(t.state, TaskState::Running)
                {
                    return Err(AgentError::TaskRunning(t_id.clone()));
                }
            }
            seen.into_iter().collect()
        } else {
            if !opts.force {
                let referencers = referencing_task_ids(&all, id);
                if !referencers.is_empty() {
                    return Err(AgentError::TaskReferenced {
                        task: id.clone(),
                        referenced_by: referencers,
                    });
                }
            }
            vec![id.clone()]
        };

        for t_id in &targets {
            self.delete(workspace_id, t_id)?;
        }
        self.prune_graph_records(workspace_id)?;
        Ok(TaskDeleteReport { deleted: targets })
    }

    /// `filter` 를 만족하는 task 중 안전하게 지울 수 있는 것만 골라낸다
    /// (순수 함수, 영속 변경 없음). Running 은 항상 후보에서 제외되고,
    /// 후보 집합 밖에서 여전히 참조되는 task 는 fixed-point 로 반복 제외한다 —
    /// 그래야 "후보 A 를 참조하는 후보 B" 처럼 후보끼리의 참조는 함께 지워지되,
    /// 후보 밖 task 의 참조는 안전하게 보존된다. dry-run 은 이 결과를 그대로
    /// 보여주면 되고, 실제 삭제는 [`Self::apply_sweep_plan`] 이 이어받는다.
    pub fn plan_sweep(
        &self,
        workspace_id: WorkspaceId,
        filter: &TaskPurgeFilter,
    ) -> Result<TaskSweepPlan> {
        let all = self.list(workspace_id)?;
        let candidates: HashSet<TaskId> = all
            .iter()
            .filter(|t| !matches!(t.state, TaskState::Running))
            .filter(|t| match &filter.states {
                None => true,
                Some(states) => states.iter().any(|s| s == t.state.name()),
            })
            .filter(|t| match filter.older_than_ms {
                None => true,
                Some(threshold) => {
                    let base = t.finished_at.unwrap_or(t.created_at);
                    filter.now_ms.saturating_sub(base) >= threshold
                }
            })
            .map(|t| t.id.clone())
            .collect();

        let mut eligible = candidates.clone();
        loop {
            let blocked: Vec<TaskId> = eligible
                .iter()
                .filter(|id| {
                    referencing_task_ids(&all, id)
                        .iter()
                        .any(|r| !eligible.contains(r))
                })
                .cloned()
                .collect();
            if blocked.is_empty() {
                break;
            }
            for id in blocked {
                eligible.remove(&id);
            }
        }

        let mut deleted: Vec<TaskId> = eligible.iter().cloned().collect();
        deleted.sort();
        let mut retained: Vec<TaskId> = candidates.difference(&eligible).cloned().collect();
        retained.sort();
        Ok(TaskSweepPlan { deleted, retained })
    }

    /// [`Self::plan_sweep`] 이 만든 계획을 실제로 적용(삭제)한다. 계획 자체가
    /// 이미 Running 제외 + 참조 안전을 보장하므로 추가 검증 없이 그대로 지운다.
    pub fn apply_sweep_plan(
        &mut self,
        workspace_id: WorkspaceId,
        plan: &TaskSweepPlan,
    ) -> Result<()> {
        for id in &plan.deleted {
            self.delete(workspace_id, id)?;
        }
        self.prune_graph_records(workspace_id)
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
mod graph_submit;
pub use graph_submit::*;
mod postprocess;

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
}

fn settle_typed_terminal(task: &mut Task, requested: TaskState) -> TaskState {
    if !task.is_typed() {
        return requested;
    }
    let failure_result = |task: &Task, failure: TaskFailure| TypedResult {
        has_output: false,
        output: super::types::TypedValue::Null,
        raw: Default::default(),
        artifacts: Vec::new(),
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
