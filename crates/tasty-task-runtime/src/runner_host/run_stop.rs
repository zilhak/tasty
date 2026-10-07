//! 종결된 task 의 프로세스를 끝내고, 끝난 것을 확인한 뒤에 점유를 놓는다.
//!
//! 취소 등으로 task 가 종결돼도 Run·후처리 프로세스는 스스로 끝나지 않는다. 그 프로세스가 아직
//! 자원을 쓰고 있을 수 있어, 생사를 모르는 동안 같은 semaphore·lease 를 다른 task 에 넘기지
//! 않는다. 프로세스 묶음을 끝내고([`super::run_group`]) 종료를 확인한 다음 tick 에서 반환한다.

use serde_json::Value;
use tasty_agent::TaskId;
use tasty_agent::runner::DispatchHandle;
use tasty_memory::{MemoryStorage, MemoryValue, Scope};

use super::{HostExecutor, handle_key, run_group};

/// 저장한 handle 레코드에서 프로세스 시작 시각을 담는 키. handle 의 `kind`·`data` 옆에 둔다.
pub(crate) const HANDLE_STARTED_AT_FIELD: &str = "started_at";

/// task 의 프로세스. 시작 시각이 없으면(옛 레코드·읽기 실패) PID 생존만으로 판단한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunProc {
    pub(crate) pid: u32,
    pub(crate) started_at: Option<u64>,
}

impl RunProc {
    pub(crate) fn is_running(&self) -> bool {
        run_group::is_running(self.pid, self.started_at)
    }

    /// 같은 프로세스가 살아 있거나 이 호스트의 Run 이 아직 끝나지 않았으면 묶음을 끝낸다.
    /// 신호를 보냈으면 `true`.
    pub(crate) fn terminate(&self) -> bool {
        run_group::terminate(self.pid, self.started_at)
    }

    /// 끝낸 묶음이 모두 끝났는가(리더와 남은 그룹 구성원).
    pub(crate) fn has_ended(&self) -> bool {
        run_group::has_ended(self.pid, self.started_at)
    }
}

/// 프로세스 handle 이면 PID.
fn process_pid(handle: &DispatchHandle) -> Option<u32> {
    match handle {
        DispatchHandle::ShellProcess { pid } | DispatchHandle::PostprocessProcess { pid, .. } => {
            Some(*pid)
        }
        _ => None,
    }
}

/// 저장할 handle 값에 그 프로세스의 시작 시각을 붙인다. handle 을 읽는 쪽은 이 키를 무시한다.
pub(crate) fn with_started_at(
    mut value: MemoryValue,
    proc: Option<&RunProc>,
    handle: &DispatchHandle,
) -> MemoryValue {
    if let (MemoryValue::Json(Value::Object(obj)), Some(proc), Some(pid)) =
        (&mut value, proc, process_pid(handle))
        && proc.pid == pid
        && let Some(t) = proc.started_at
    {
        obj.insert(HANDLE_STARTED_AT_FIELD.into(), t.into());
    }
    value
}

/// 레코드 JSON 에서 프로세스를 읽는다.
pub(crate) fn process_of_record(value: &Value) -> Option<RunProc> {
    let started_at = value.get(HANDLE_STARTED_AT_FIELD).and_then(Value::as_u64);
    let handle: DispatchHandle = serde_json::from_value(value.clone()).ok()?;
    Some(RunProc {
        pid: process_pid(&handle)?,
        started_at,
    })
}

/// 저장된 handle 의 프로세스. 프로세스 handle 이 아니면 없다.
pub(crate) fn stored_process(
    mem: &dyn MemoryStorage,
    workspace_id: u32,
    task_id: &str,
) -> Option<RunProc> {
    let entry = mem
        .get(&Scope::Workspace(workspace_id), &handle_key(task_id))
        .ok()
        .flatten()?;
    let MemoryValue::Json(value) = entry.value else {
        return None;
    };
    process_of_record(&value)
}

impl HostExecutor {
    /// task 의 프로세스가 아직 살아 있으면 묶음을 끝내고 종료 대기에 넣는다(`true`).
    pub(super) fn stop_run(&mut self, task_id: &TaskId) -> bool {
        let Some(proc) = self.run_procs.get(task_id) else {
            return false;
        };
        if !proc.terminate() {
            return false;
        }
        tracing::info!(
            "agent task {task_id}: ended; stopping its process group (pid {}) before releasing its holdings",
            proc.pid
        );
        if !self.stopping_runs.contains(task_id) {
            self.stopping_runs.push(task_id.clone());
        }
        true
    }

    /// 끝낸 프로세스 중 종료를 확인한 task. 남은 것은 다음 tick 에 다시 본다.
    pub(super) fn stopped_runs(&mut self) -> Vec<TaskId> {
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.stopping_runs)
            .into_iter()
            .partition(|id| self.run_procs.get(id).is_none_or(|p| p.has_ended()));
        self.stopping_runs = waiting;
        done
    }

    /// 재시작 뒤 넘겨받은 Run 의 프로세스가 살아 있는가. 넘겨받은 기록이 없으면 PID 만 본다.
    pub(super) fn restored_run_alive(&self, pid: u32) -> bool {
        let started_at = self
            .run_procs
            .values()
            .find(|p| p.pid == pid)
            .and_then(|p| p.started_at);
        run_group::is_running(pid, started_at)
    }

    /// 재시작 뒤 넘겨받은 회차의 프로세스를 기록한다. 취소하면 이 기록으로 끝낸다.
    pub(crate) fn adopt_process(&mut self, task_id: &TaskId, proc: RunProc) {
        self.run_procs.insert(task_id.clone(), proc);
    }
}
