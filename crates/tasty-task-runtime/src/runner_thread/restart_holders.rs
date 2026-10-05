//! Reconcile task-owned semaphore and lease holders before runner restart.

use super::{RunnerContext, now_ms};
use tasty_agent::{LeaseStore, SemaphoreStore, Task, TaskResult, TaskState, TaskStore};
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
                // 실패하면 permit이 영구히 묶인다. 바로 아래에서 작업을 실패로 마킹하므로
                // 나중에 해제할 주체가 없다. 상태 처리는 그대로 이어간다.
                if let Err(e) = sem.release(workspace_id, name, holder) {
                    tracing::warn!(
                        "semaphore '{name}' release for dead holder {holder} failed: {e}"
                    );
                }
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            fail_restarted_task(
                &mut store,
                workspace_id,
                task_id,
                now,
                "purge",
                &mut transitioned,
            );
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
                // 실패하면 lease가 영구히 묶인다. 바로 아래에서 작업을 실패로 마킹하므로
                // 나중에 회수할 주체가 없다. 상태 처리는 그대로 이어간다.
                if let Err(e) = lstore.release(workspace_id, resource, holder) {
                    tracing::warn!(
                        "lease '{resource}' release for dead holder {holder} failed: {e}"
                    );
                }
            }
        }
        let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
        for (task_id, _, _) in &candidates {
            fail_restarted_task(
                &mut store,
                workspace_id,
                task_id,
                now,
                "purge(lease)",
                &mut transitioned,
            );
        }
        transitioned
    });
    ctx.fire_terminal_tasks(workspace_id, transitioned);
    tracing::info!(
        "agent runner ws{workspace_id}: processed {} stale lease holder candidate(s) during startup cleanup",
        candidates.len()
    );
}

// Failed result persistence must not prevent the state transition or downstream notifications.
fn fail_restarted_task(
    store: &mut TaskStore<'_>,
    workspace_id: u32,
    task_id: &tasty_agent::TaskId,
    now: u64,
    context: &str,
    transitioned: &mut Vec<Task>,
) {
    if let Err(e) = store.set_result(
        workspace_id,
        task_id,
        TaskResult {
            exit_code: None,
            output: None,
            error: Some("host restart".to_string()),
        },
    ) {
        tracing::warn!("{context} set_result for {task_id} failed: {e}");
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
        Err(e) => tracing::warn!("{context} set_state for {task_id} failed: {e}"),
    }
}
