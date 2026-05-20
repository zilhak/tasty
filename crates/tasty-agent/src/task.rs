//! Task primitive — DAG, 상태 머신, memory 영속.
//!
//! 의도적으로 IPC/GUI와 독립적이다. 호스트는 본 모듈의 API를 호출해 task 모델을
//! 영속하고, 별도 스케줄러가 `Ready` 상태 task를 실제로 실행한다 (`ClaudeSpawn`은
//! `claude.spawn` IPC, `Run`은 `tab.create + cmd`, `Custom`은 임의 IPC dispatch,
//! `Reduce`는 본 크레이트 내부 reducer로). 실행 완료 신호가 들어오면 호스트가
//! `TaskStore::set_state`로 진행시키고 downstream의 readiness가 갱신된다.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tasty_core::model::{SurfaceId, WorkspaceId};
use tasty_memory::{ListOpts, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::{AgentError, Result};

/// Task의 고유 식별자. 형식 `t-<timestamp_ms>-<seq>` (예: `t-1716800000123-7`).
/// 호스트가 본 크레이트 외부에서 임의 문자열로 생성해도 무방하지만, 본 모듈의
/// 헬퍼는 위 형식으로 발급한다.
pub type TaskId = String;

/// Task의 state 머신.
///
/// 변환 규칙:
/// - `Waiting → Ready` (의존성 모두 Succeeded)
/// - `Waiting → Skipped` (의존성 중 하나 Failed, downstream skip 모드)
/// - `Waiting → Cancelled` (사용자/abort)
/// - `Ready → Running` (스케줄러가 실행 시작)
/// - `Ready → Cancelled`
/// - `Running → Succeeded`
/// - `Running → Failed`
/// - `Running → Cancelled`
/// - `Unknown → Ready` (사용자 명시 retry)
/// - `Unknown → Cancelled`
///
/// 재시작 후 `Running` 상태였던 task는 호스트가 `Unknown`으로 표시한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskState {
    Waiting,
    Ready,
    Running,
    Succeeded,
    Failed { error: String },
    Cancelled,
    Skipped,
    Unknown,
}

impl TaskState {
    pub fn name(&self) -> &'static str {
        match self {
            TaskState::Waiting => "waiting",
            TaskState::Ready => "ready",
            TaskState::Running => "running",
            TaskState::Succeeded => "succeeded",
            TaskState::Failed { .. } => "failed",
            TaskState::Cancelled => "cancelled",
            TaskState::Skipped => "skipped",
            TaskState::Unknown => "unknown",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Succeeded
                | TaskState::Failed { .. }
                | TaskState::Cancelled
                | TaskState::Skipped
        )
    }
}

/// Task가 실패했을 때 downstream 처리 정책.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OnFailure {
    /// downstream 모두 `Skipped`로.
    Abort,
    /// downstream을 정상 진행 (의존성 실패를 성공처럼 취급).
    ContinueDownstream,
    /// 다른 task를 fallback으로 실행. 그 fallback이 Succeed하면 downstream이 정상 진행.
    Fallback { task: TaskId },
}

impl Default for OnFailure {
    fn default() -> Self {
        OnFailure::Abort
    }
}

/// Task가 실행할 동작.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskCommand {
    /// `claude.spawn` IPC를 호출해 자식 Claude를 띄운다.
    ClaudeSpawn {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nickname: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_surface: Option<SurfaceId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<String>, // "vertical" | "horizontal"
    },
    /// 새 terminal surface에서 일반 명령 실행.
    Run {
        command: Vec<String>,
        workspace_id: WorkspaceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
    },
    /// 임의 IPC 메서드 호출 위임 (호출자가 해당 메서드의 권한을 보유해야 함).
    Custom {
        ipc_method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// 다른 task의 결과를 합성.
    Reduce {
        inputs: Vec<TaskId>,
        strategy: ReducerStrategy,
    },
}

/// Reducer 합성 전략.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReducerStrategy {
    FirstSuccess,
    All,
    MergeJson,
    ConcatText,
    /// shell 명령으로 결과 배열 stdin 전달, stdout이 최종.
    Custom { command: String },
}

/// Task 실행 결과. `Succeeded`/`Failed` 상태에서만 채워진다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskResult {
    /// 종료 코드 (Run의 exit_code, ClaudeSpawn의 wait 결과 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 실행 산출물 (claude의 surface id, Custom의 IPC 응답 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    /// 에러 사유 (Failed 상태에서만).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 영속되는 Task 한 레코드.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub command: TaskCommand,
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
    pub state: TaskState,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// 워크스페이스에 속한 task들의 그래프 뷰. 사이클 검출, downstream 계산용.
pub struct TaskGraph<'a> {
    tasks: HashMap<&'a TaskId, &'a Task>,
}

impl<'a> TaskGraph<'a> {
    pub fn build(tasks: &'a [Task]) -> Self {
        let mut map = HashMap::new();
        for t in tasks {
            map.insert(&t.id, t);
        }
        Self { tasks: map }
    }

    /// 사이클 검출. 발견 시 `Err(DependencyCycle)`.
    pub fn detect_cycles(&self) -> Result<()> {
        // DFS 3-color: 0=white(unvisited), 1=gray(in stack), 2=black(done).
        let mut color: HashMap<&TaskId, u8> = HashMap::new();
        let mut stack_path: Vec<&TaskId> = Vec::new();

        for &start in self.tasks.keys() {
            if color.get(start).copied().unwrap_or(0) != 0 {
                continue;
            }
            self.dfs_cycle(start, &mut color, &mut stack_path)?;
        }
        Ok(())
    }

    fn dfs_cycle(
        &self,
        node: &'a TaskId,
        color: &mut HashMap<&'a TaskId, u8>,
        stack: &mut Vec<&'a TaskId>,
    ) -> Result<()> {
        color.insert(node, 1);
        stack.push(node);
        if let Some(task) = self.tasks.get(node) {
            for dep in &task.depends_on {
                let dep_ref = self
                    .tasks
                    .get_key_value(dep)
                    .map(|(k, _)| *k)
                    .ok_or_else(|| AgentError::UnknownDependency(dep.clone()))?;
                match color.get(dep_ref).copied().unwrap_or(0) {
                    0 => self.dfs_cycle(dep_ref, color, stack)?,
                    1 => {
                        // cycle: stack의 dep_ref 이후 부분을 반환
                        let from = stack
                            .iter()
                            .position(|t| *t == dep_ref)
                            .unwrap_or(0);
                        let cycle: Vec<TaskId> =
                            stack[from..].iter().map(|t| (*t).clone()).collect();
                        return Err(AgentError::DependencyCycle(cycle));
                    }
                    _ => {}
                }
            }
        }
        color.insert(node, 2);
        stack.pop();
        Ok(())
    }

    /// `task_id`의 직접 downstream (이 task에 의존하는 task들).
    pub fn downstream_of(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut out = Vec::new();
        for (id, t) in &self.tasks {
            if t.depends_on.iter().any(|d| d == task_id) {
                out.push((*id).clone());
            }
        }
        out
    }

    /// `task_id`의 transitive downstream.
    pub fn transitive_downstream(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut seen: HashSet<TaskId> = HashSet::new();
        let mut queue: Vec<TaskId> = self.downstream_of(task_id);
        let mut out = Vec::new();
        while let Some(cur) = queue.pop() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            out.push(cur.clone());
            queue.extend(self.downstream_of(&cur));
        }
        out
    }

    /// `task_id`의 의존성 상태를 평가해 `Ready`로 진행 가능한지 판단.
    /// 반환:
    /// - `Some(TaskState::Ready)` — 모든 dep Succeeded
    /// - `Some(TaskState::Skipped)` — dep 중 Failed/Cancelled/Skipped 존재
    /// - `None` — 아직 대기 (dep에 미완료가 있음)
    pub fn evaluate_readiness(&self, task_id: &TaskId) -> Option<TaskState> {
        let task = self.tasks.get(task_id)?;
        if task.depends_on.is_empty() {
            return Some(TaskState::Ready);
        }
        let mut any_failed = false;
        for dep_id in &task.depends_on {
            let dep = match self.tasks.get(dep_id) {
                Some(d) => d,
                None => return None,
            };
            match &dep.state {
                TaskState::Succeeded => {}
                TaskState::Failed { .. } | TaskState::Cancelled | TaskState::Skipped => {
                    // dep 가 Fallback{f} 정책이면 f 의 상태를 본다 — f 성공 = dep 충족.
                    if let OnFailure::Fallback { task: fb_id } = &dep.on_failure {
                        match self.tasks.get(fb_id).map(|t| &t.state) {
                            Some(TaskState::Succeeded) => continue,
                            Some(
                                TaskState::Failed { .. }
                                | TaskState::Cancelled
                                | TaskState::Skipped,
                            ) => any_failed = true,
                            // fallback 진행 중(또는 미존재) → 대기.
                            _ => return None,
                        }
                    } else {
                        any_failed = true;
                    }
                }
                _ => return None,
            }
        }
        if any_failed {
            Some(TaskState::Skipped)
        } else {
            Some(TaskState::Ready)
        }
    }
}

// ============================================================
// 영속 (memory-backed)
// ============================================================

/// memory key prefix.
const TASK_KEY_PREFIX: &str = "tasty.agent.task.";

fn task_key(id: &TaskId) -> String {
    format!("{TASK_KEY_PREFIX}{id}")
}

/// `MemoryStore` 위에 얹은 Task 영속 + state 머신.
///
/// 본 store는 빌려 쓰는 형태. 호스트가 `MemoryStore`의 lock을 잡은 상태에서
/// 임시로 wrap해 호출한다.
pub struct TaskStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
    seq: &'a AtomicU64,
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
    pub fn create(
        &mut self,
        workspace_id: WorkspaceId,
        name: impl Into<String>,
        command: TaskCommand,
        depends_on: Vec<TaskId>,
        on_failure: OnFailure,
        metadata: serde_json::Value,
        now_ms: u64,
    ) -> Result<Task> {
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
            name: name.into(),
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
                Some(TaskState::Skipped) => {
                    apply_on_failure(&d_task, &all_now)
                }
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
                if let Some(mut d) = self.get(workspace_id, &d_id)? {
                    if matches!(
                        d.state,
                        TaskState::Skipped | TaskState::Failed { .. } | TaskState::Cancelled
                    ) {
                        d.state = TaskState::Waiting;
                        d.started_at = None;
                        d.finished_at = None;
                        d.result = None;
                        self.put(&d)?;
                    }
                }
            }
            // downstream 모두 갱신했으니 cascade 한번 더
            self.cascade_downstream(workspace_id, id)?;
        }

        Ok(task)
    }
}

/// 상태 전이가 valid한지.
fn is_valid_transition(from: &TaskState, to: &TaskState) -> bool {
    use TaskState::*;
    match (from, to) {
        (Waiting, Ready)
        | (Waiting, Skipped)
        | (Waiting, Cancelled)
        | (Ready, Running)
        | (Ready, Cancelled)
        | (Running, Succeeded)
        | (Running, Failed { .. })
        | (Running, Cancelled)
        | (Unknown, Ready)
        | (Unknown, Cancelled)
        | (Unknown, Waiting) => true,
        // retry 경로는 별도 메서드에서 처리. 직접 set_state로는 거부.
        _ => false,
    }
}

/// on_failure 정책에 따라 downstream task의 목표 상태를 결정.
/// 호출 시점 `task`는 dep 중 하나가 실패해 `evaluate_readiness`가 `Skipped`를
/// 반환한 상태로 가정한다.
fn apply_on_failure(task: &Task, _all: &[Task]) -> Option<TaskState> {
    match &task.on_failure {
        OnFailure::Abort => Some(TaskState::Skipped),
        OnFailure::ContinueDownstream => Some(TaskState::Ready),
        // Fallback은 호스트가 fallback task를 별도 트리거해야 함. 본 task는 일단
        // Waiting을 유지하고, fallback이 Succeed하면 그때 호스트가 다시 평가.
        OnFailure::Fallback { .. } => None,
    }
}


#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
