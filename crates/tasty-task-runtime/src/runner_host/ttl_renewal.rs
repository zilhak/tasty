//! 러너가 지켜보는 task 의 lease·semaphore TTL 갱신.
//!
//! task 가 끝나기 전에 TTL 이 지나면 다른 holder 가 같은 자원을 얻어 두 실행이 함께 쓴다. 러너는
//! task 의 lease·permit 을 쥐고 있는 동안(`release_resources` 전까지, 끝낸 프로세스의 종료를
//! 기다리는 동안 포함) TTL 의 절반이 지날 때마다 만료 시각을 지금 + TTL 로 늦춘다. 절반 주기는 다음
//! 갱신이 tick 지연·저장소 대기로 늦어져도 남은 절반 안에 들면 만료되지 않게 하는 여유다. 갱신은
//! tick 시작(`maintain`)에 하므로 TTL 은 tick 간격의 두 배 이상이어야 한다(작업 생성·제출 때
//! 검사한다, [`check_holding_ttls`]). 러너가 꺼진 동안에는 아무도 갱신하지 않는다. 갱신하지 못해
//! 점유를 잃으면 task 조회에 남긴다([`super::holding_warning`]).

use serde_json::{Value, json};
use tasty_agent::{LeaseStore, OnFailure, SemaphoreStore, Task, TaskId};
use tasty_memory::HOST_OWNER;

use super::HostExecutor;

/// 작업 생성·제출 때 받는 점유 TTL 의 하한(1초). 갱신은 TTL 의 절반이 지난 뒤 처음 오는 tick 에
/// 한다. 그 tick 은 늦어도 절반 + tick 간격에 오므로, TTL 이 tick 간격의 두 배 이상이어야 만료
/// 전에 갱신된다. 이보다 짧으면 갱신 사이에 만료돼 다른 holder 가 같은 자원을 얻는다.
pub(crate) const MIN_HOLDING_TTL_MS: u64 =
    2 * crate::runner_thread::TICK_INTERVAL.as_millis() as u64;

/// 작업 metadata(와 inline fallback 의 metadata)의 lease·semaphore `ttl_ms` 가 하한보다 짧으면 거절한다.
pub(crate) fn check_holding_ttls(
    task: &str,
    metadata: &Value,
    on_failure: &OnFailure,
) -> Result<(), String> {
    for kind in [Holding::Lease, Holding::Permit] {
        if let Some(ttl) = metadata
            .get(kind.label())
            .and_then(|m| m.get("ttl_ms"))
            .and_then(Value::as_u64)
            && ttl < MIN_HOLDING_TTL_MS
        {
            return Err(format!(
                "task {task}: metadata.{}.ttl_ms {ttl} is below the minimum {MIN_HOLDING_TTL_MS} ms; \
                 the runner renews it once per tick and a shorter TTL can run out between renewals",
                kind.label()
            ));
        }
    }
    if let OnFailure::Fallback {
        inline: Some(spec), ..
    } = on_failure
    {
        check_holding_ttls(
            &format!("{task} (inline fallback {})", spec.name),
            &spec.metadata,
            &spec.on_failure,
        )?;
    }
    Ok(())
}

/// TTL 을 두고 쥐는 점유의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Holding {
    Lease,
    Permit,
}

impl Holding {
    fn label(self) -> &'static str {
        match self {
            Holding::Lease => "lease",
            Holding::Permit => "semaphore",
        }
    }
}

/// TTL 을 둔 점유 하나의 갱신 상태.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TtlRenewal {
    ttl_ms: u64,
    /// 마지막으로 만료 시각을 정한 때(epoch ms). 0 이면 다음 tick 에 바로 갱신한다.
    renewed_at: u64,
}

/// task metadata 의 점유 TTL.
pub(super) fn holding_ttl(task: &Task, kind: Holding) -> Option<u64> {
    task.metadata.get(kind.label())?.get("ttl_ms")?.as_u64()
}

impl HostExecutor {
    /// 얻은(또는 넘겨받은) 점유를 갱신 대상으로 둔다. `renewed_at` 이 0 이면 다음 tick 에 갱신한다.
    pub(super) fn track_renewal(
        &mut self,
        task_id: &TaskId,
        kind: Holding,
        ttl_ms: u64,
        renewed_at: u64,
    ) {
        self.ttl_renewals
            .insert((task_id.clone(), kind), TtlRenewal { ttl_ms, renewed_at });
    }

    /// 갱신 주기가 된 점유의 만료 시각을 늦춘다. 이미 다른 holder 가 쥔 자원은 되찾지 않는다.
    pub(super) fn renew_holdings(&mut self) {
        let now = self.holding_clock.now_ms();
        let due: Vec<(TaskId, Holding)> = self
            .ttl_renewals
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.renewed_at) >= r.ttl_ms / 2)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            let (task_id, kind) = &key;
            let held = match kind {
                Holding::Lease => self.held_leases.get(task_id),
                Holding::Permit => self.held_permits.get(task_id),
            };
            let (Some(r), Some((ws, name, holder))) =
                (self.ttl_renewals.get(&key).copied(), held.cloned())
            else {
                self.ttl_renewals.remove(&key);
                continue;
            };
            let renewed = self.ctx.with_memory(|mem| match kind {
                Holding::Lease => {
                    LeaseStore::new(mem, HOST_OWNER).renew(ws, &name, &holder, r.ttl_ms, now)
                }
                Holding::Permit => {
                    SemaphoreStore::new(mem, HOST_OWNER).renew(ws, &name, &holder, r.ttl_ms, now)
                }
            });
            match renewed {
                Ok(true) => self.track_renewal(task_id, *kind, r.ttl_ms, now),
                Ok(false) => {
                    let message = format!(
                        "{} '{name}' is no longer held by '{holder}': its TTL ran out before it was renewed, so another holder may be using it while this task keeps running",
                        kind.label()
                    );
                    tracing::warn!("agent task {task_id}: {message}; not renewing it");
                    super::holding_warning::record_holding_warning(
                        &self.ctx,
                        ws,
                        task_id,
                        json!({
                            "kind": kind.label(),
                            "name": name,
                            "holder": holder,
                            "at_ms": now,
                            "message": message,
                        }),
                    );
                    self.ttl_renewals.remove(&key);
                }
                // 다음 tick 에 다시 시도한다.
                Err(e) => {
                    tracing::warn!("agent task {task_id}: {} '{name}' renew: {e}", kind.label())
                }
            }
        }
    }

    /// task 의 갱신 대상을 모두 지운다(점유를 반환할 때).
    pub(super) fn forget_renewals(&mut self, task_id: &TaskId) {
        self.ttl_renewals.retain(|(id, _), _| id != task_id);
    }
}
