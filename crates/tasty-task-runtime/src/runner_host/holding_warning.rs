//! 러너가 TTL 을 갱신하지 못해 task 가 점유를 잃은 기록. task 조회(`holding_warnings`)로 드러낸다.
//!
//! 점유를 잃어도 task 는 계속 실행된다(프로세스를 멈출 근거가 없다). 다른 holder 가 같은 자원을
//! 함께 쓰고 있을 수 있음을 호출자가 알 수 있게 남긴다. task 를 지울 때 함께 지운다.

use serde_json::Value;
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};

use super::RunnerContext;

const HOLDING_WARNING_KEY_PREFIX: &str = "tasty.agent.holding_warning.";
/// 한 task 에 남기는 기록 수 상한. 갱신 실패 뒤에는 그 점유의 갱신을 멈추므로 회차마다 많아야
/// 점유 종류 수만큼 생긴다. 재시도가 거듭돼도 끝없이 늘지 않게 오래된 것부터 버린다.
const MAX_HOLDING_WARNINGS: usize = 16;

fn holding_warning_key(task_id: &str) -> String {
    format!("{HOLDING_WARNING_KEY_PREFIX}{task_id}")
}

/// task 의 점유 상실 기록. 오래된 것부터.
pub(crate) fn holding_warnings(
    mem: &dyn MemoryStorage,
    workspace_id: u32,
    task_id: &str,
) -> Vec<Value> {
    let entry = mem
        .get(
            &Scope::Workspace(workspace_id),
            &holding_warning_key(task_id),
        )
        .ok()
        .flatten();
    match entry.map(|e| e.value) {
        Some(MemoryValue::Json(Value::Array(items))) => items,
        _ => Vec::new(),
    }
}

/// 기록을 하나 더한다.
pub(crate) fn record_holding_warning(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
    warning: Value,
) {
    let res = ctx.with_memory(|mem| {
        let mut items = holding_warnings(&*mem, workspace_id, task_id);
        items.push(warning);
        let excess = items.len().saturating_sub(MAX_HOLDING_WARNINGS);
        items.drain(..excess);
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &holding_warning_key(task_id),
            &MemoryValue::Json(Value::Array(items)),
            &PutOpts::default(),
        )
    });
    if let Err(e) = res {
        tracing::warn!("agent task {task_id}: holding warning record: {e}");
    }
}

pub(crate) fn evict_holding_warnings(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &holding_warning_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict holding warning {task_id}: {e}");
    }
}
