//! 러너가 지켜보지 않는 동안 종결된 task 의 정리. 러너가 꺼진 동안 취소한 task 가 대상이다.
//!
//! 남은 Run·후처리 프로세스를 끝내고 끝난 것을 확인한 뒤에 handle 을 지우고 점유를 반환한다.
//! 생사를 모르는 프로세스가 쥔 자원을 다른 task 에 넘기지 않기 위해서다. [`KILL_CONFIRM_WAIT`]
//! 안에 확인하지 못하면 둘 다 남겨 다음 reload(러너 시작·부팅)가 다시 시도한다.

use std::time::{Duration, Instant};

use tasty_agent::Task;
use tasty_memory::{HOST_OWNER, MemoryValue, Scope};

use super::RunnerContext;
use crate::runner_host::{RunProc, handle_key, process_of_record, release_own_holdings};

/// 프로세스 묶음을 끝낸 뒤 종료를 확인할 때까지 기다리는 상한. SIGKILL·job 종료는 보통 수 ms
/// 안에 끝나고 회수(같은 호스트는 watcher, 재시작 뒤는 init)도 곧 따른다. 정리를 부르는 IPC·부팅이
/// 오래 멈추지 않도록 짧게 둔다. 넘으면 다음 reload 로 미룬다.
pub(crate) const KILL_CONFIRM_WAIT: Duration = Duration::from_secs(2);
const KILL_CONFIRM_STEP: Duration = Duration::from_millis(10);

/// 종결된 task 의 저장된 handle 을 정리한다. 정리를 마쳤거나 정리할 handle 이 없으면 `true`.
pub(crate) fn settle_ended_task(ctx: &RunnerContext, workspace_id: u32, task: &Task) -> bool {
    let scope = Scope::Workspace(workspace_id);
    let stored = ctx.with_memory(|mem| {
        mem.get(&scope, &handle_key(&task.id))
            .ok()
            .flatten()
            .map(|e| match e.value {
                MemoryValue::Json(v) => process_of_record(&v),
                _ => None,
            })
    });
    let Some(proc) = stored else {
        return true;
    };
    if let Some(proc) = proc
        && proc.terminate()
        && !wait_until_ended(proc, KILL_CONFIRM_WAIT)
    {
        tracing::warn!(
            "agent task {} (ws {workspace_id}): pid {} did not end within {KILL_CONFIRM_WAIT:?} after it was killed; its holdings stay until a later reload confirms the exit",
            task.id,
            proc.pid
        );
        return false;
    }
    ctx.with_memory(|mem| {
        mem.delete(HOST_OWNER, &scope, &handle_key(&task.id), None)
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "failed to evict an ended task handle; reload will retry");
            });
        release_own_holdings(mem, workspace_id, task);
    });
    true
}

fn wait_until_ended(proc: RunProc, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if !proc.is_running() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(KILL_CONFIRM_STEP);
    }
}
