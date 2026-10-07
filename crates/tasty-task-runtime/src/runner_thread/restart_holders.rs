//! Reconcile task-owned semaphore and lease holders before runner restart.

use super::{RunnerContext, now_ms};
use crate::runner_host::{own_lease, own_semaphore, resumes_as_a_run};
use tasty_agent::{LeaseStore, SemaphoreStore, Task, TaskResult, TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, MemoryStorage};

/// 재시작 정리 대상: Running 이고 자기 id 로 점유를 쥔 task. 저장된 Run handle 로 다시 감시할
/// 회차는 뺀다. 그 프로세스는 살아 있을 수 있어 handle 복원이 점유를 유지하거나, 끝난 것을
/// 확인한 뒤 반환한다([`crate::runner_host::release_own_holdings`]).
fn restart_candidates(
    ctx: &RunnerContext,
    workspace_id: u32,
    holding: fn(&Task) -> Option<String>,
) -> Vec<(String, String, String)> {
    ctx.with_memory(|mem| {
        let seq = ctx.agent_seq.clone();
        let listed = TaskStore::new(&mut *mem, HOST_OWNER, seq.as_ref()).list(workspace_id);
        let Ok(tasks) = listed else {
            return Vec::new();
        };
        let mem: &dyn MemoryStorage = mem;
        tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Running))
            .filter(|t| !resumes_as_a_run(mem, workspace_id, t))
            .filter_map(|t| {
                let held = holding(&t)?;
                let holder = t.id.clone();
                Some((t.id, held, holder))
            })
            .collect()
    })
}

/// 재시작 정리 대상 중 semaphore 를 쥔 task 의 permit 을 반환하고 host restart 실패로 표시한다.
/// holder를 생략하면 task ID로 본다. 별도 holder를 지정한 외부 점유는 러너가 회수하지 않는다.
pub(super) fn purge_stale_semaphore_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates = restart_candidates(ctx, workspace_id, own_semaphore);
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

/// metadata.lease.resource가 있고 holder가 task ID인 재시작 정리 대상만 정리한다.
/// candidates만 지정한 pool은 여기서 실제 획득 자원을 복원하지 않는다.
pub(super) fn purge_stale_lease_holders(ctx: &RunnerContext, workspace_id: u32) {
    let now = now_ms();
    let candidates = restart_candidates(ctx, workspace_id, own_lease);
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

#[cfg(test)]
#[path = "restart_holders_tests.rs"]
mod tests;
