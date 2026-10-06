//! 실행 회차를 다루는 러너 쪽 연결이다. 완료 보고를 한 경로로 기록하고, 저장하는 실행 handle 에
//! dispatch 회차를 남겨 재시작 복구가 그 회차로 보고하게 한다.

use serde_json::Value;
use tasty_agent::task::{Completion, CompletionReceipt, TaskStore};
use tasty_agent::{AgentError, Task, TaskId};
use tasty_memory::{HOST_OWNER, MemoryValue};

use super::RunnerContext;

/// 저장한 handle 레코드에서 dispatch 회차 id 를 담는 키. handle 의 `kind`·`data` 옆에 둔다.
pub(crate) const HANDLE_ATTEMPT_FIELD: &str = "attempt_id";

impl RunnerContext {
    /// 완료 보고를 기록하고, 락을 놓은 뒤 이 보고로 종결된 task 의 대기자를 깨운다.
    /// runner·훅·만료·재시작 복구가 모두 이 경로를 쓴다.
    pub(crate) fn complete_task(
        &self,
        workspace_id: u32,
        task_id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<CompletionReceipt, AgentError> {
        let receipt = self.with_memory(|mem| {
            TaskStore::new(mem, HOST_OWNER, self.agent_seq.as_ref()).complete(
                workspace_id,
                task_id,
                completion,
                now_ms,
            )
        })?;
        // 같은 보고의 재전송은 이미 알린 종결을 다시 알리지 않는다.
        let own = (!receipt.duplicate).then(|| receipt.task.clone());
        self.fire_terminal_tasks(
            workspace_id,
            own.into_iter().chain(receipt.transitioned.iter().cloned()),
        );
        Ok(receipt)
    }
}

/// dispatch 직후 Running 전이가 만들 v2 회차 id. v1 task 는 없다.
pub(crate) fn dispatch_attempt(task: &Task) -> Option<String> {
    tasty_agent::task::attempt::next_attempt(task, 0).map(|a| a.id)
}

/// 저장할 handle 값에 dispatch 회차를 붙인다. handle 을 읽는 쪽은 이 키를 무시한다.
pub(crate) fn handle_value(mut handle: Value, attempt: Option<&str>) -> MemoryValue {
    if let (Some(attempt), Some(obj)) = (attempt, handle.as_object_mut()) {
        obj.insert(HANDLE_ATTEMPT_FIELD.into(), attempt.into());
    }
    MemoryValue::Json(handle)
}
