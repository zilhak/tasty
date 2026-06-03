//! Task store wrapper. handler 의 `core.with_memory + TaskStore::new` 조립을
//! 본 모듈로 흡수. `agent_seq` 의 시퀀스 공유는 그대로 유지.

use tasty_agent::task::TaskCreateOpts;
use tasty_agent::{AgentError, ReducerInput, Task, TaskId, TaskResult, TaskState, TaskStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;
use crate::core::CoreState;

impl Core {
    /// Task 생성 — `TaskStore::create` wrapper.
    pub(crate) fn task_create(
        &self,
        engine: &CoreState,
        opts: TaskCreateOpts,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.create(opts)
        })
    }

    /// Task 목록.
    pub(crate) fn task_list(
        &self,
        engine: &CoreState,
        workspace_id: u32,
    ) -> Result<Vec<Task>, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.list(workspace_id)
        })
    }

    /// Task 단건 조회.
    pub(crate) fn task_get(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
    ) -> Result<Option<Task>, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.get(workspace_id, task_id)
        })
    }

    /// Task 취소 — downstream cascade 포함.
    pub(crate) fn task_cancel(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = engine.agent_seq.clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.cancel(workspace_id, task_id, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(engine, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(engine, workspace_id, d);
            }
        }
        result
    }

    /// S5: task state 가 종결 (Succeeded/Failed/Cancelled/Skipped) 이면 waker hub 에
    /// fire. set_state / cancel / runner thread 등 *모든* terminal 진입 경로에서
    /// 호출되어야 누락 없음 (R-5 회피).
    fn fire_waker_if_terminal(&self, engine: &CoreState, workspace_id: u32, task: &Task) {
        if !task.state.is_terminal() {
            return;
        }
        engine.task_waker_hub.fire(
            workspace_id,
            &task.id,
            crate::core::agent::task_waker::TerminalSnapshot {
                state: task.state.clone(),
                result: task.result.clone(),
            },
        );
    }

    /// Task retry — 옵션에 따라 downstream reset.
    pub(crate) fn task_retry(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.retry(workspace_id, task_id, reset_downstream, now_ms)
        })
    }

    /// Task state 강제 전이 — runner 가 dispatch / poll 결과에 따라 호출.
    /// downstream cascade 도 함께 수행. 반환: (갱신된 자기, 자동 전이된 downstream).
    pub(crate) fn task_set_state(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>), AgentError> {
        let seq = engine.agent_seq.clone();
        let result = self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_state(workspace_id, task_id, new_state, now_ms)
        });
        if let Ok((ref task, ref downstream)) = result {
            self.fire_waker_if_terminal(engine, workspace_id, task);
            for d in downstream {
                self.fire_waker_if_terminal(engine, workspace_id, d);
            }
        }
        result
    }

    /// Task result 영속. set_state(Succeeded/Failed) 직전에 호출.
    pub(crate) fn task_set_result(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        task_id: &TaskId,
        result: TaskResult,
    ) -> Result<Task, AgentError> {
        let seq = engine.agent_seq.clone();
        self.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            store.set_result(workspace_id, task_id, result)
        })
    }

    /// Reducer 단계 1: 입력 task 들의 결과를 `ReducerInput` 형태로 수집.
    /// 실제 reducer / shell I/O 는 handler 가 *memory lock 바깥에서* 실행.
    pub(crate) fn task_reduce_collect(
        &self,
        engine: &CoreState,
        workspace_id: u32,
        inputs: &[TaskId],
    ) -> Result<Vec<ReducerInput>, AgentError> {
        let seq = engine.agent_seq.clone();
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
                out.push(ReducerInput { succeeded, output });
            }
            Ok(out)
        })
    }
}
