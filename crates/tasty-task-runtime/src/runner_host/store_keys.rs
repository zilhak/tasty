//! runner 가 workspace 메모리에 남기는 실행 handle·실행 결과의 키와 handle 읽기.

use tasty_agent::runner::DispatchHandle;
use tasty_memory::{MemoryValue, Scope};

use super::RunnerContext;

/// 재시작 뒤 실행 중인 작업을 복원할 workspace별 handle 키. 즉시 끝나는 handle은 저장하지 않는다.
pub(crate) const HANDLE_KEY_PREFIX: &str = "tasty.agent.handle.";

pub(crate) fn handle_key(task_id: &str) -> String {
    format!("{HANDLE_KEY_PREFIX}{task_id}")
}

/// IPC 조회가 외부 완료 신호의 wait_key·deadline도 보여줄 수 있도록 저장된 handle을 읽는다.
pub(crate) fn load_dispatch_handle(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
) -> Option<DispatchHandle> {
    let scope = Scope::Workspace(workspace_id);
    ctx.with_memory(|mem| {
        let entry = mem.get(&scope, &handle_key(task_id)).ok().flatten()?;
        match entry.value {
            MemoryValue::Json(v) => serde_json::from_value(v).ok(),
            _ => None,
        }
    })
}

/// 자식 종료와 출력 수집 뒤 기록하는 결과 키. 기록 전에 호스트가 종료되거나 저장이 실패하면 남지 않을 수 있다.
const RUN_RESULT_KEY_PREFIX: &str = "tasty.agent.run_result.";

pub(crate) fn run_result_key(task_id: &str) -> String {
    format!("{RUN_RESULT_KEY_PREFIX}{task_id}")
}
