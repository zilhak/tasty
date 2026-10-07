//! 러너가 지켜보지 않는 동안 종결된 task 의 정리. 러너가 꺼진 동안 취소한 task 가 대상이다.
//!
//! 남은 Run·후처리 프로세스를 끝내고 끝난 것을 확인한 뒤에 handle 을 지우고 점유를 반환한다.
//! 생사를 모르는 프로세스가 쥔 자원을 다른 task 에 넘기지 않기 위해서다. 확인하지 못하면 둘 다
//! 남기고, 러너가 켜져 있는 동안 tick 마다 다시 본다([`settle_unwatched_handles`]). 정리는 확인한
//! 회차의 handle 일 때만 지우고 반환한다. 옛 회차의 handle 이 남은 동안 그 task 의 새 회차는
//! 시작하지 않는다(`HostExecutor::dispatch`).
//!
//! 취소 IPC 는 기다리지 않는다([`settle_ended_tasks_in_background`]). IPC 는 앱의 처리 경로에서
//! 차례로 처리되므로, SIGKILL 이 늦게 듣는 프로세스(D 상태 등)가 다른 IPC 를 막지 않게 한다.
//! 확인과 반환은 별도 스레드가 하고, 그 전까지 점유는 그대로 남는다.

use std::time::{Duration, Instant};

use serde_json::Value;
use tasty_agent::{Task, TaskId, TaskState, TaskStore};
use tasty_memory::{HOST_OWNER, ListOpts, MemoryValue, Scope};

use super::RunnerContext;
use crate::runner_host::{
    HANDLE_ATTEMPT_FIELD, HANDLE_KEY_PREFIX, RunProc, handle_key, process_of_record,
    release_own_holdings,
};

/// 프로세스 묶음을 끝낸 뒤 종료를 확인할 때까지 기다리는 상한. SIGKILL·job 종료는 보통 수 ms
/// 안에 끝나고 회수(같은 호스트는 watcher, 재시작 뒤는 init)도 곧 따른다. 정리를 부르는 IPC·부팅이
/// 오래 멈추지 않도록 짧게 둔다. 넘으면 다음 reload 로 미룬다.
pub(crate) const KILL_CONFIRM_WAIT: Duration = Duration::from_secs(2);
/// 취소 IPC 뒤 백그라운드에서 종료를 기다리는 상한. 그 안에 끝나지 않는 프로세스는 러너 tick 의
/// 재확인과 reload 에 맡긴다. 스레드가 끝없이 남지 않게 하는 상한이며, 넘어도 점유는 남는다.
pub(crate) const BACKGROUND_CONFIRM_WAIT: Duration = Duration::from_secs(600);
const KILL_CONFIRM_STEP: Duration = Duration::from_millis(10);

/// 종결된 task 의 저장된 handle 을 정리한다. 정리를 마쳤거나 정리할 handle 이 없으면 `true`.
/// reload 경로에서 쓰며 최대 [`KILL_CONFIRM_WAIT`] 를 기다린다.
pub(crate) fn settle_ended_task(ctx: &RunnerContext, workspace_id: u32, task: &Task) -> bool {
    settle_within(ctx, workspace_id, task, KILL_CONFIRM_WAIT)
}

/// 취소한 task 들을 별도 스레드에서 정리한다. 호출은 기다리지 않는다.
pub(crate) fn settle_ended_tasks_in_background(
    ctx: RunnerContext,
    workspace_id: u32,
    tasks: Vec<Task>,
) {
    let spawned = std::thread::Builder::new()
        .name(format!("agent-cancel-settle-ws{workspace_id}"))
        .spawn(move || {
            for task in &tasks {
                settle_within(&ctx, workspace_id, task, BACKGROUND_CONFIRM_WAIT);
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(
            "agent cancel (ws {workspace_id}): settle thread spawn failed: {e}; the next reload stops the processes and returns the holdings"
        );
    }
}

fn settle_within(ctx: &RunnerContext, workspace_id: u32, task: &Task, limit: Duration) -> bool {
    let Some(stored) = read_stored(ctx, workspace_id, &task.id) else {
        return true;
    };
    // 시작 시각이 없는 옛 handle 은 같은 프로세스인지 알 수 없어 기다리지 않는다(이전 동작).
    if let Some(proc) = stored.proc.filter(|p| p.started_at.is_some()) {
        // 리더가 이미 끝났어도 그룹 구성원이 남았을 수 있어 신호를 보냈는지와 관계없이 확인한다.
        proc.terminate();
        if !wait_until_ended(proc, limit) {
            tracing::warn!(
                "agent task {} (ws {workspace_id}): pid {} did not end within {limit:?} after it was killed; its holdings stay until the runner confirms the exit",
                task.id,
                proc.pid
            );
            return false;
        }
    }
    finalize(ctx, workspace_id, &task.id, &stored);
    true
}

/// 러너가 지켜보지 않는 handle 을 tick 마다 다시 본다. 대상은 task 가 없거나 Running 이 아닌
/// handle 이다(취소된 회차, 재시도로 Ready 가 된 task 의 옛 회차). 남은 프로세스 묶음을 끝내고,
/// 끝난 것을 확인한 tick 에 정리한다. 기다리지 않으므로 확인이 늦어도 tick 을 막지 않고, 백그라운드
/// 확인의 상한이나 reload 의 2초를 넘긴 handle 도 러너가 켜져 있는 동안 계속 다시 본다.
pub(crate) fn settle_unwatched_handles(
    ctx: &RunnerContext,
    workspace_id: u32,
    watched: impl Fn(&TaskId) -> bool,
) {
    let scope = Scope::Workspace(workspace_id);
    let entries = ctx.with_memory(|mem| {
        let opts = ListOpts {
            prefix: Some(HANDLE_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        mem.list(&scope, &opts).unwrap_or_default()
    });
    for e in entries {
        let Some(task_id) = e.key.strip_prefix(HANDLE_KEY_PREFIX).map(str::to_string) else {
            continue;
        };
        if watched(&task_id) {
            continue;
        }
        let running = ctx.with_memory(|mem| {
            TaskStore::new(mem, HOST_OWNER, ctx.agent_seq.as_ref())
                .get(workspace_id, &task_id)
                .ok()
                .flatten()
                .is_some_and(|t| matches!(t.state, TaskState::Running))
        });
        // Running 인데 러너가 모르는 handle 은 reload 가 정한다(여기서 끝내지 않는다).
        if running {
            continue;
        }
        let stored = match &e.value {
            MemoryValue::Json(v) => Stored::of(v),
            _ => Stored {
                proc: None,
                attempt: None,
            },
        };
        // 시작 시각이 없는 옛 handle 은 같은 프로세스인지 알 수 없어 기다리지 않는다(이전 동작).
        if let Some(proc) = stored.proc.filter(|p| p.started_at.is_some()) {
            proc.terminate();
            if !proc.has_ended() {
                continue;
            }
        }
        finalize(ctx, workspace_id, &task_id, &stored);
    }
}

/// handle 이 남은 task id. Running 이 아닌 task 에 남았으면 이전 회차의 종료를 확인하는 중이다.
pub(crate) fn stored_handle_ids(
    mem: &dyn tasty_memory::MemoryStorage,
    workspace_id: u32,
) -> std::collections::HashSet<TaskId> {
    let opts = ListOpts {
        prefix: Some(HANDLE_KEY_PREFIX.to_string()),
        ..Default::default()
    };
    mem.list(&Scope::Workspace(workspace_id), &opts)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e.key.strip_prefix(HANDLE_KEY_PREFIX).map(str::to_string))
        .collect()
}

/// 저장된 handle 한 건의 프로세스와 회차. 정리할 handle 이 확인한 그 handle 인지 가리는 데 쓴다
/// (v1 task 는 회차 id 가 없어 프로세스로 가린다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    proc: Option<RunProc>,
    attempt: Option<String>,
}

impl Stored {
    pub(crate) fn of(value: &Value) -> Self {
        Self {
            proc: process_of_record(value),
            attempt: record_attempt(value),
        }
    }
}

fn record_attempt(value: &Value) -> Option<String> {
    value
        .get(HANDLE_ATTEMPT_FIELD)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn read_stored(ctx: &RunnerContext, workspace_id: u32, task_id: &str) -> Option<Stored> {
    ctx.with_memory(|mem| {
        let entry = mem
            .get(&Scope::Workspace(workspace_id), &handle_key(task_id))
            .ok()
            .flatten()?;
        Some(match entry.value {
            MemoryValue::Json(v) => Stored::of(&v),
            _ => Stored {
                proc: None,
                attempt: None,
            },
        })
    })
}

/// 확인한 handle(`expected`, 회차와 프로세스)이 아직 그대로면 지우고, task 가 Running 이 아니면 그
/// task 가 자기 id 로 쥔 점유를 반환한다. 한 잠금 안에서 한다. handle 이 이미 지워졌거나 다른 회차·
/// 프로세스의 것이면 아무것도 하지 않는다. 그 점유와 handle 은 다른 정리나 새 회차의 것이다. 옛
/// 회차의 handle 이 남은 동안 새 회차는 시작하지 않으므로(dispatch 가 미룬다) 같은 holder id 의
/// 점유는 옛 회차의 것이다.
pub(crate) fn finalize(ctx: &RunnerContext, workspace_id: u32, task_id: &str, expected: &Stored) {
    let scope = Scope::Workspace(workspace_id);
    let key = handle_key(task_id);
    ctx.with_memory(|mem| {
        let current = match mem.get(&scope, &key) {
            Ok(Some(entry)) => match entry.value {
                MemoryValue::Json(v) => Stored::of(&v),
                _ => Stored {
                    proc: None,
                    attempt: None,
                },
            },
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%error, "agent task {task_id}: handle read failed; the runner retries");
                return;
            }
        };
        if &current != expected {
            return;
        }
        if let Err(error) = mem.delete(HOST_OWNER, &scope, &key, None) {
            tracing::warn!(%error, "failed to evict an ended task handle; the runner retries");
            return;
        }
        let task = TaskStore::new(&mut *mem, HOST_OWNER, ctx.agent_seq.as_ref())
            .get(workspace_id, &task_id.to_string())
            .ok()
            .flatten();
        if let Some(task) = task
            && !matches!(task.state, TaskState::Running)
        {
            release_own_holdings(mem, workspace_id, &task);
        }
    });
}

fn wait_until_ended(proc: RunProc, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if proc.has_ended() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(KILL_CONFIRM_STEP);
    }
}
