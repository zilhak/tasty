//! task 가 metadata 로 자기 id 를 holder 삼아 쥐는 semaphore·lease 와, 재시작 뒤 그 점유를
//! 누가 정리하는가. 부팅 정리·handle 복원·복원한 회차의 확정이 같은 판정을 쓴다.
//!
//! 재시작 뒤에도 Run·후처리 프로세스는 살아 있을 수 있다. 생사를 모르는 동안 같은 자원을 다른
//! task 에 넘기지 않도록, 저장된 handle 로 다시 감시할 회차는 부팅 정리가 점유를 풀지 않는다.
//! 프로세스가 살아 있으면 점유를 유지하고, 끝난 것을 확인한 뒤(저장 결과로 확정하거나 결과
//! 불명) 반환한다.

use serde_json::Value;
use tasty_agent::runner::DispatchHandle;
use tasty_agent::task::postprocess::PostprocessPhase;
use tasty_agent::{LeaseStore, SemaphoreStore, Task};
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, Scope};

use super::{HANDLE_ATTEMPT_FIELD, HostExecutor, handle_key};

/// task 가 자기 id 로 쥐는 semaphore 이름. 다른 holder 를 지정한 외부 점유는 러너가 다루지 않는다.
pub(crate) fn own_semaphore(task: &Task) -> Option<String> {
    own_holding(task, "semaphore", "name")
}

/// task 가 자기 id 로 쥐는 lease 자원. `candidates` 만 지정한 pool 은 얻은 자원을 metadata 에
/// 남기지 않아 여기서 찾지 못한다.
pub(crate) fn own_lease(task: &Task) -> Option<String> {
    own_holding(task, "lease", "resource")
}

fn own_holding(task: &Task, kind: &str, field: &str) -> Option<String> {
    let meta = task.metadata.get(kind)?.as_object()?;
    let holder = meta.get("holder").and_then(Value::as_str);
    if holder.is_some_and(|h| h != task.id) {
        return None;
    }
    Some(meta.get(field)?.as_str()?.to_string())
}

/// 저장된 handle(지금 회차의 것)로 다시 이어 갈 Running task 인가. 본 작업 중이면 Run handle,
/// 후처리 실행을 시작한(`Started`) 회차면 후처리 handle 이다. 후처리를 기다리는(`Pending`, 재시도
/// 대기 포함) 회차는 실행 중인 프로세스가 없지만 예약대로 후처리를 실행하므로 점유를 쥔 채
/// 이어 간다(점유가 없는 같은 단계의 task 와 같다). 그때 저장된 handle 은 직전 본 작업이나
/// 후처리 실행의 것이다.
pub(crate) fn resumes_after_restart(
    mem: &dyn MemoryStorage,
    workspace_id: u32,
    task: &Task,
) -> bool {
    let attempt = task.attempt.as_ref();
    let phase = attempt
        .and_then(|a| a.postprocess.as_ref())
        .map(|p| p.phase);
    let Some(entry) = mem
        .get(&Scope::Workspace(workspace_id), &handle_key(&task.id))
        .ok()
        .flatten()
    else {
        return false;
    };
    let MemoryValue::Json(value) = entry.value else {
        return false;
    };
    let saved_attempt = value.get(HANDLE_ATTEMPT_FIELD).and_then(Value::as_str);
    if saved_attempt != attempt.map(|a| a.id.as_str()) {
        return false;
    }
    match (serde_json::from_value::<DispatchHandle>(value), phase) {
        (Ok(DispatchHandle::ShellProcess { .. }), None) => true,
        (
            Ok(DispatchHandle::PostprocessProcess { run, .. }),
            Some(PostprocessPhase::Started { run: started, .. }),
        ) => run == started,
        (
            Ok(DispatchHandle::ShellProcess { .. } | DispatchHandle::PostprocessProcess { .. }),
            Some(PostprocessPhase::Pending { .. }),
        ) => true,
        _ => false,
    }
}

/// 끝난 것을 확인한 회차의 점유를 반환한다. 그 사이 TTL 이 지나 다른 holder 가 쥔 자원은
/// holder 가 달라 건드리지 않는다.
pub(crate) fn release_own_holdings(mem: &mut dyn MemoryStorage, workspace_id: u32, task: &Task) {
    if let Some(name) = own_semaphore(task)
        && let Err(e) =
            SemaphoreStore::new(&mut *mem, HOST_OWNER).release(workspace_id, &name, &task.id)
    {
        tracing::warn!("semaphore '{name}' release for {} failed: {e}", task.id);
    }
    if let Some(resource) = own_lease(task)
        && let Err(e) =
            LeaseStore::new(&mut *mem, HOST_OWNER).release(workspace_id, &resource, &task.id)
    {
        tracing::warn!("lease '{resource}' release for {} failed: {e}", task.id);
    }
}

impl HostExecutor {
    /// 재시작 뒤 복원한 회차(Run·후처리)의 점유와 저장된 handle 을 이 executor 의 기록으로
    /// 되살린다. 그래야 회차가 끝날 때 `release_resources` 가 반환한다.
    pub(crate) fn adopt_restored_run(&mut self, workspace_id: u32, task: &Task) {
        self.held_handles.insert(task.id.clone(), workspace_id);
        let proc = self
            .ctx
            .with_memory(|mem| super::stored_process(mem, workspace_id, &task.id));
        if let Some(proc) = proc {
            self.adopt_process(&task.id, proc);
        }
        use super::ttl_renewal::{Holding, holding_ttl};
        if let Some(name) = own_semaphore(task) {
            self.held_permits
                .insert(task.id.clone(), (workspace_id, name, task.id.clone()));
            // 내려가 있던 동안 지난 시간을 모르니 다음 tick 에 바로 늦춘다.
            if let Some(ttl) = holding_ttl(task, Holding::Permit) {
                self.track_renewal(&task.id, Holding::Permit, ttl, 0);
            }
        }
        if let Some(resource) = own_lease(task) {
            self.held_leases
                .insert(task.id.clone(), (workspace_id, resource, task.id.clone()));
            if let Some(ttl) = holding_ttl(task, Holding::Lease) {
                self.track_renewal(&task.id, Holding::Lease, ttl, 0);
            }
        }
    }
}
