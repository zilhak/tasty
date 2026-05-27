//! `TaskStore` — task 의 persistent CRUD + state 전이.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use tasty_memory::{ListOpts, MemoryStore, MemoryValue, PutOpts, Scope};
use tasty_utils::id::WorkspaceId;

use super::{
    OnFailure, TASK_KEY_PREFIX, Task, TaskCommand, TaskGraph, TaskId, TaskResult, TaskState,
    apply_on_failure, is_valid_transition, task_key,
};
use crate::{AgentError, Result};

pub struct TaskStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
    seq: &'a AtomicU64,
}

/// [`TaskStore::create`] 인자 묶음.
///
/// `name` 은 String 으로 받음 — 기존의 `impl Into<String>` 은 generic 이지만 Opts struct
/// 안에 두려면 명시 타입 필요. 호출자가 `&str` 이면 `.into()` 또는 `.to_string()` 호출.
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
    pub fn new(mem: &'a mut MemoryStore, owner: impl Into<String>, seq: &'a AtomicU64) -> Self {
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

    /// task 영속. 신규/갱신 모두 동일 (overwrite).
    pub fn put(&mut self, task: &Task) -> Result<()> {
        let scope = Scope::Workspace(task.workspace_id);
        let key = task_key(&task.id);
        let value = MemoryValue::Json(serde_json::to_value(task)?);
        self.mem
            .put(&self.owner, &scope, &key, &value, &PutOpts::default())?;
        Ok(())
    }

    /// 단건 조회.
    pub fn get(&self, workspace_id: WorkspaceId, id: &TaskId) -> Result<Option<Task>> {
        let scope = Scope::Workspace(workspace_id);
        let entry = self.mem.get(&scope, &task_key(id))?;
        match entry {
            Some(e) => match e.value {
                MemoryValue::Json(v) => Ok(Some(serde_json::from_value(v)?)),
                _ => Err(AgentError::InvalidArgument(format!(
                    "task entry is not json: {id}"
                ))),
            },
            None => Ok(None),
        }
    }

    /// 워크스페이스 전체 task 목록.
    pub fn list(&self, workspace_id: WorkspaceId) -> Result<Vec<Task>> {
        let scope = Scope::Workspace(workspace_id);
        let opts = ListOpts {
            prefix: Some(TASK_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&scope, &opts)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            if let MemoryValue::Json(v) = e.value {
                out.push(serde_json::from_value(v)?);
            }
        }
        Ok(out)
    }

    /// task 삭제 (드물게 사용; 보통은 Cancelled 상태로 유지).
    pub fn delete(&mut self, workspace_id: WorkspaceId, id: &TaskId) -> Result<()> {
        let scope = Scope::Workspace(workspace_id);
        self.mem.delete(&self.owner, &scope, &task_key(id), None)?;
        Ok(())
    }

    /// 신규 task 생성. 사이클 검출 + 초기 state 계산 후 영속.
    /// `now_ms`는 호스트가 주입 (테스트 결정성).
    pub fn create(&mut self, opts: TaskCreateOpts) -> Result<Task> {
        let TaskCreateOpts {
            workspace_id,
            name,
            command,
            depends_on,
            on_failure,
            metadata,
            now_ms,
        } = opts;
        let id = self.new_id(now_ms);
        let mut existing = self.list(workspace_id)?;

        // unknown dep 검출
        let known: HashSet<&TaskId> = existing.iter().map(|t| &t.id).collect();
        for dep in &depends_on {
            if !known.contains(dep) {
                return Err(AgentError::UnknownDependency(dep.clone()));
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
        };

        // 임시로 그래프에 포함시켜 사이클 검출
        existing.push(new_task.clone());
        {
            let graph = TaskGraph::build(&existing);
            graph.detect_cycles()?;
            // 초기 readiness 계산
            if let Some(state) = graph.evaluate_readiness(&new_task.id) {
                new_task.state = state;
            }
        }

        self.put(&new_task)?;
        Ok(new_task)
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
        match new_state {
            TaskState::Running => {
                task.started_at = Some(now_ms);
            }
            TaskState::Succeeded
            | TaskState::Failed { .. }
            | TaskState::Cancelled
            | TaskState::Skipped => {
                task.finished_at = Some(now_ms);
            }
            _ => {}
        }
        let became = task.state.clone();
        task.state = new_state.clone();
        self.put(&task)?;

        let mut transitioned = Vec::new();

        // Failed 전이 + on_failure=Fallback{f} → f 를 자동 Ready 로 (가능하면).
        if matches!(new_state, TaskState::Failed { .. })
            && let OnFailure::Fallback { task: fb_id } = task.on_failure.clone()
            && let Some(mut fb) = self.get(workspace_id, &fb_id)?
        {
            // fallback task 의 dep readiness 를 새로 평가.
            let all_now = self.list(workspace_id)?;
            if let Some(target) = TaskGraph::build(&all_now).evaluate_readiness(&fb_id)
                && target != TaskState::Waiting
                && is_valid_transition(&fb.state, &target)
            {
                fb.state = target;
                self.put(&fb)?;
                transitioned.push(fb);
            }
        }

        // downstream 재평가
        transitioned.extend(self.cascade_downstream(workspace_id, id)?);

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
                .filter(|t| matches!(&t.on_failure, OnFailure::Fallback { task } if task == id))
                .map(|t| t.id.clone())
                .collect();
            for main_id in parent_main_ids {
                transitioned.extend(self.cascade_downstream(workspace_id, &main_id)?);
            }
        }
        let _ = became;
        Ok((task, transitioned))
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
        task.result = Some(result);
        self.put(&task)?;
        Ok(task)
    }

    /// `task_id`의 모든 transitive downstream에서 `Waiting` 상태인 것들을 평가해
    /// 가능하면 `Ready/Skipped`로 전이. on_failure 정책도 함께 적용.
    fn cascade_downstream(
        &mut self,
        workspace_id: WorkspaceId,
        task_id: &TaskId,
    ) -> Result<Vec<Task>> {
        let all = self.list(workspace_id)?;
        let downstream_ids = {
            let graph = TaskGraph::build(&all);
            graph.transitive_downstream(task_id)
        };
        let mut updated = Vec::new();
        // BFS 순서대로: 각 단계마다 최신 상태로 다시 list/build.
        for d_id in downstream_ids {
            let all_now = self.list(workspace_id)?;
            let graph = TaskGraph::build(&all_now);
            let d_task = match self.get(workspace_id, &d_id)? {
                Some(t) => t,
                None => continue,
            };
            if !matches!(d_task.state, TaskState::Waiting) {
                continue;
            }
            let target = match graph.evaluate_readiness(&d_id) {
                Some(TaskState::Skipped) => apply_on_failure(&d_task, &all_now),
                other => other,
            };
            if let Some(next) = target
                && next != TaskState::Waiting
                && is_valid_transition(&d_task.state, &next)
            {
                let mut nt = d_task;
                nt.state = next;
                self.put(&nt)?;
                updated.push(nt);
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
        task.state = TaskState::Waiting;
        task.started_at = None;
        task.finished_at = None;
        task.result = None;
        let _ = now_ms; // 향후 retried_at 필드 후보. 현재는 미사용.
        self.put(&task)?;

        // readiness 즉시 평가
        let all = self.list(workspace_id)?;
        if let Some(next) = TaskGraph::build(&all).evaluate_readiness(id)
            && next != TaskState::Waiting
        {
            let mut nt = task.clone();
            nt.state = next;
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
                    self.put(&d)?;
                }
            }
            // downstream 모두 갱신했으니 cascade 한번 더
            self.cascade_downstream(workspace_id, id)?;
        }

        Ok(task)
    }
}
