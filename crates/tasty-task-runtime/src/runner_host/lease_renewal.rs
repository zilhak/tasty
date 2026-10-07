//! 러너가 지켜보는 task 의 lease TTL 갱신.
//!
//! task 가 끝나기 전에 TTL 이 지나면 다른 holder 가 같은 자원을 얻어 두 실행이 함께 쓴다. 러너는
//! task 의 lease 를 쥐고 있는 동안(`release_resources` 전까지, 끝난 프로세스의 종료를 기다리는
//! 동안 포함) TTL 의 절반이 지날 때마다 만료 시각을 지금 + TTL 로 늦춘다. 절반 주기는 다음 갱신이
//! tick 지연·저장소 대기로 늦어져도 남은 절반 안에 들면 만료되지 않게 하는 여유다. 갱신은 tick
//! 시작(`maintain`)에 하므로 TTL 이 tick 간격의 두 배보다 짧으면 갱신 사이에 만료될 수 있다.
//! 러너가 꺼진 동안에는 아무도 갱신하지 않는다.

use tasty_agent::{LeaseStore, Task, TaskId};
use tasty_memory::HOST_OWNER;

use super::{HostExecutor, now_ms};

/// TTL 을 둔 lease 하나의 갱신 상태.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeaseRenewal {
    ttl_ms: u64,
    /// 마지막으로 만료 시각을 정한 때(epoch ms). 0 이면 다음 tick 에 바로 갱신한다.
    renewed_at: u64,
}

/// task metadata 의 lease TTL.
pub(super) fn lease_ttl(task: &Task) -> Option<u64> {
    task.metadata.get("lease")?.get("ttl_ms")?.as_u64()
}

impl HostExecutor {
    /// 얻은(또는 넘겨받은) lease 를 갱신 대상으로 둔다. `renewed_at` 이 0 이면 다음 tick 에 갱신한다.
    pub(super) fn track_lease_renewal(&mut self, task_id: &TaskId, ttl_ms: u64, renewed_at: u64) {
        self.lease_renewals
            .insert(task_id.clone(), LeaseRenewal { ttl_ms, renewed_at });
    }

    /// 갱신 주기가 된 lease 의 만료 시각을 늦춘다. 이미 다른 holder 가 쥔 자원은 되찾지 않는다.
    pub(super) fn renew_leases(&mut self) {
        let now = now_ms();
        let due: Vec<TaskId> = self
            .lease_renewals
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.renewed_at) >= r.ttl_ms / 2)
            .map(|(id, _)| id.clone())
            .collect();
        for task_id in due {
            let (Some(r), Some((ws, resource, holder))) = (
                self.lease_renewals.get(&task_id).copied(),
                self.held_leases.get(&task_id).cloned(),
            ) else {
                self.lease_renewals.remove(&task_id);
                continue;
            };
            let renewed = self.ctx.with_memory(|mem| {
                LeaseStore::new(mem, HOST_OWNER).renew(ws, &resource, &holder, r.ttl_ms, now)
            });
            match renewed {
                Ok(true) => self.track_lease_renewal(&task_id, r.ttl_ms, now),
                Ok(false) => {
                    tracing::warn!(
                        "agent task {task_id}: lease '{resource}' is no longer held by '{holder}' (its TTL ran out before it was renewed); not renewing it"
                    );
                    self.lease_renewals.remove(&task_id);
                }
                // 다음 tick 에 다시 시도한다.
                Err(e) => tracing::warn!("agent task {task_id}: lease '{resource}' renew: {e}"),
            }
        }
    }
}
