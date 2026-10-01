//! Reconcile task-owned semaphore and lease holders before runner restart.

use super::{RunnerContext, now_ms};
use tasty_agent::{LeaseStore, SemaphoreStore, TaskResult, TaskState, TaskStore};
use tasty_memory::HOST_OWNER;

/// Running 작업 중 semaphore 이름이 있고 holder가 task ID인 항목을 정리한다.
/// holder를 생략하면 task ID로 본다. 점유 해제 후 작업을 host restart 실패로 표시하도록 시도한다.
pub(super) fn purge_stale_semaphore_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates: Vec<(String, String, String)> = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let Ok(tasks) = store.list(workspace_id) else {
            return Vec::new();
        };
        tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .filter_map(|t| {
                let meta = t.metadata.get("semaphore")?.as_object()?;
                let name = meta.get("name")?.as_str()?.to_string();
                let holder = meta
                    .get("holder")
                    .and_then(|v| v.as_str())
                    .unwrap_or(t.id.as_str())
                    .to_string();
                // 별도 holder를 지정한 외부 점유는 러너가 회수하지 않는다.
                if holder != *t.id.as_str() {
                    return None;
                }
                Some((t.id, name, holder))
            })
            .collect()
    });
    if candidates.is_empty() {
        return;
    }
    let transitioned = ctx.with_memory(|mem| {
        let mut transitioned = Vec::new();
        let seq = ctx.agent_seq.clone();
        {
            let mut sem = SemaphoreStore::new(mem, HOST_OWNER);
            for (_task_id, name, holder) in &candidates {
                let _ = sem.release(workspace_id, name, holder); // 해제 실패도 여기서는 무시하고 작업 상태 처리를 계속한다.
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            if let Err(e) = store.set_result(
                workspace_id,
                task_id,
                TaskResult {
                    exit_code: None,
                    output: None,
                    error: Some("host restart".to_string()),
                },
            ) {
                tracing::warn!("purge set_result for {task_id} failed: {e}");
            }
            match store.set_state(
                workspace_id,
                task_id,
                TaskState::Failed {
                    error: "host restart".to_string(),
                },
                now,
            ) {
                Ok((task, downstream)) => {
                    transitioned.push(task);
                    transitioned.extend(downstream);
                }
                Err(e) => tracing::warn!("purge set_state for {task_id} failed: {e}"),
            }
        }
        transitioned
    });
    ctx.fire_terminal_tasks(workspace_id, transitioned);
    tracing::info!(
        "agent runner ws{workspace_id}: processed {} stale semaphore holder candidate(s) during startup cleanup",
        candidates.len()
    );
}

/// metadata.lease.resource가 있고 holder가 task ID인 Running 작업만 정리한다.
/// candidates만 지정한 pool은 여기서 실제 획득 자원을 복원하지 않는다.
pub(super) fn purge_stale_lease_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates: Vec<(String, String, String)> = ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        let Ok(tasks) = store.list(workspace_id) else {
            return Vec::new();
        };
        tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .filter_map(|t| {
                let meta = t.metadata.get("lease")?.as_object()?;
                let resource = meta.get("resource")?.as_str()?.to_string();
                let holder = meta
                    .get("holder")
                    .and_then(|v| v.as_str())
                    .unwrap_or(t.id.as_str())
                    .to_string();
                if holder != *t.id.as_str() {
                    return None;
                }
                Some((t.id, resource, holder))
            })
            .collect()
    });
    if candidates.is_empty() {
        return;
    }
    let transitioned = ctx.with_memory(|mem| {
        let mut transitioned = Vec::new();
        let seq = ctx.agent_seq.clone();
        {
            let mut lstore = LeaseStore::new(mem, HOST_OWNER);
            for (_task_id, resource, holder) in &candidates {
                let _ = lstore.release(workspace_id, resource, holder); // 해제 실패도 여기서는 무시하고 작업 상태 처리를 계속한다.
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            if let Err(e) = store.set_result(
                workspace_id,
                task_id,
                TaskResult {
                    exit_code: None,
                    output: None,
                    error: Some("host restart".to_string()),
                },
            ) {
                tracing::warn!("purge(lease) set_result for {task_id} failed: {e}");
            }
            match store.set_state(
                workspace_id,
                task_id,
                TaskState::Failed {
                    error: "host restart".to_string(),
                },
                now,
            ) {
                Ok((task, downstream)) => {
                    transitioned.push(task);
                    transitioned.extend(downstream);
                }
                Err(e) => tracing::warn!("purge(lease) set_state for {task_id} failed: {e}"),
            }
        }
        transitioned
    });
    ctx.fire_terminal_tasks(workspace_id, transitioned);
    tracing::info!(
        "agent runner ws{workspace_id}: processed {} stale lease holder candidate(s) during startup cleanup",
        candidates.len()
    );
}
