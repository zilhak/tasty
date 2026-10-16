//! 시작한 뒤 응답 대기 상한을 넘긴 Custom 요청의 늦은 응답을 기다린다. agent task 의 spawn·tell 응답
//! 대기도 여기 둔다([`super::agent_spawn`], [`super::agent`]).
//!
//! 실행 요청(예: `claude.spawn`)은 다시 보내면 두 번 실행될 수 있다. 상한이 지나도 요청을 다시
//! 보내지 않고, 보관한 응답 통로에서 늦은 응답을 받아 완료 전략의 handle 로 이어 간다.

use std::collections::HashMap;

use serde_json::Value;
use tasty_agent::runner::{DispatchHandle, PollOutcome};
use tasty_agent::{Task, TaskCommand, TaskId};
use tasty_ipc::host_call::{InjectError, LateAnswer};

use super::{HostExecutor, dispatch_attempt};

/// 늦은 응답이 결과를 확정하지 못한 Unknown 사유의 머리말.
pub(crate) const DISPATCH_OUTCOME_UNKNOWN: &str = "dispatch outcome unknown";

/// 응답을 기다리는 요청 하나. 응답이 오면 같은 회차의 인자로 완료 전략을 잇는다.
struct Pending {
    answer: LateAnswer,
    task: Task,
    params: Value,
    started_ms: u64,
}

#[derive(Default)]
pub(super) struct LateAnswers {
    pending: HashMap<TaskId, Pending>,
    /// 늦은 응답 뒤 이어 간 handle. 러너는 계속 [`DispatchHandle::AwaitDispatchAnswer`] 를
    /// 들고 있으므로 poll 을 여기 handle 로 넘긴다.
    pub(super) continued: HashMap<TaskId, DispatchHandle>,
    /// 응답을 기다리는 agent task 의 `<provider>.spawn`. 러너는 `AwaitDispatchAnswer` 를 들고 있다.
    pub(super) spawns: HashMap<TaskId, super::agent_spawn::PendingSpawn>,
    /// 시작한 뒤 상한을 넘긴 agent task 의 `<provider>.tell`. 러너는 `AgentTurn` 을 들고 있다.
    pub(super) tells: HashMap<TaskId, LateAnswer>,
}

impl LateAnswers {
    pub(super) fn forget(&mut self, task_id: &TaskId) {
        self.pending.remove(task_id);
        self.continued.remove(task_id);
        self.tells.remove(task_id);
        self.spawns.remove(task_id);
    }
}

impl HostExecutor {
    /// 응답 통로를 보관하고 늦은 응답을 기다리는 handle 을 돌려준다.
    pub(super) fn park_late_answer(
        &mut self,
        task: &Task,
        ipc_method: &str,
        params: Value,
        answer: LateAnswer,
        started_ms: u64,
    ) -> DispatchHandle {
        tracing::warn!(
            "agent task {}: Custom '{ipc_method}' started but did not answer within {:?}; \
             waiting for its late answer instead of sending it again",
            task.id,
            answer.timeout()
        );
        self.late.continued.remove(&task.id);
        self.late.pending.insert(
            task.id.clone(),
            Pending {
                answer,
                task: task.clone(),
                params,
                started_ms,
            },
        );
        DispatchHandle::AwaitDispatchAnswer {
            workspace_id: task.workspace_id,
            task_id: task.id.clone(),
            ipc_method: ipc_method.to_string(),
        }
    }

    pub(super) fn poll_late_answer(
        &mut self,
        workspace_id: u32,
        task_id: &TaskId,
        ipc_method: &str,
    ) -> PollOutcome {
        if let Some(handle) = self.late.continued.get(task_id).cloned() {
            return self.poll_handle(&handle);
        }
        if let Some(outcome) = self.poll_agent_spawn(task_id) {
            return outcome;
        }
        let Some(pending) = self.late.pending.get(task_id) else {
            // 통로는 이 executor 에만 있다. 없으면(재시작 뒤 복원 등) 요청의 결과를 알 수 없다.
            return PollOutcome::Lost(format!(
                "{DISPATCH_OUTCOME_UNKNOWN}: '{ipc_method}' started but its answer channel \
                 is gone; check whether it took effect before retrying"
            ));
        };
        let Some(answer) = pending.answer.try_take() else {
            return PollOutcome::Active;
        };
        let Some(pending) = self.late.pending.remove(task_id) else {
            return PollOutcome::Active;
        };
        let value = match answer {
            Ok(v) => v,
            Err(InjectError::Disconnected) => {
                return PollOutcome::Lost(format!(
                    "{DISPATCH_OUTCOME_UNKNOWN}: Custom '{ipc_method}' started but its answer \
                     channel closed without an answer; check whether it took effect before retrying"
                ));
            }
            Err(e) => return PollOutcome::Failed(format!("Custom '{ipc_method}': {e}")),
        };
        tracing::info!("agent task {task_id}: Custom '{ipc_method}' answered late");
        let TaskCommand::Custom { poll, .. } = &pending.task.command else {
            return PollOutcome::Failed(format!(
                "Custom '{ipc_method}': the late answer belongs to a task that is not Custom"
            ));
        };
        let handle = match self.custom_after_answer(
            &pending.task,
            ipc_method,
            &pending.params,
            poll.as_deref(),
            value,
            pending.started_ms,
        ) {
            Ok(h) => h,
            Err(e) => return PollOutcome::Failed(e),
        };
        match handle {
            DispatchHandle::CustomImmediate(r) => PollOutcome::Done(r),
            DispatchHandle::ImmediateFail(e) => PollOutcome::Failed(e),
            handle => {
                // 재시작 뒤에도 이어 간 handle 로 복원하도록 저장된 handle 을 바꾼다.
                let attempt = dispatch_attempt(&pending.task);
                self.persist_handle(workspace_id, task_id, &handle, attempt.as_deref());
                self.late.continued.insert(task_id.clone(), handle.clone());
                self.poll_handle(&handle)
            }
        }
    }
}

#[cfg(test)]
#[path = "late_answer_tests.rs"]
mod tests;
