//! 실행 전 점유(lease·semaphore) 획득과 종결 뒤 점유·handle 반환.

use tasty_agent::{AgentError, ElasticSpec, LeaseMode, LeaseStore, SemaphoreStore, Task, TaskId};
use tasty_memory::HOST_OWNER;
use tasty_memory::writer::Blocked;

use super::{HostExecutor, ttl_renewal};

impl HostExecutor {
    /// semaphore metadata가 없으면 None, 얻었으면 Some(true), 부족하면 Some(false)다. 잘못된 name·저장소 오류는 Err다.
    pub(super) fn try_acquire_semaphore(&mut self, task: &Task) -> Result<Option<bool>, String> {
        let Some(meta) = task.metadata.get("semaphore").and_then(|v| v.as_object()) else {
            return Ok(None);
        };
        let name = meta
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "semaphore metadata: missing 'name'".to_string())?;
        let holder = meta
            .get("holder")
            .and_then(|v| v.as_str())
            .unwrap_or(task.id.as_str());
        // TTL을 생략하면 자동 만료시키지 않는다.
        let ttl_ms = meta.get("ttl_ms").and_then(|v| v.as_u64());
        let name = name.to_string();
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let now = self.holding_clock.now_ms();
        let (n, h) = (name.clone(), holder.clone());
        let result = self.ctx.store(move |mem| {
            let mut store = SemaphoreStore::new(mem, HOST_OWNER);
            store
                .acquire(ws, &n, &h, ttl_ms, now)
                .map(|o| o.acquired)
                .map_err(|e| e.to_string())
        });
        let acquired = match result {
            Ok(acquired) => acquired?,
            // 접수되지 않았거나 확정되지 않았다. 저장 오류와 같이 다룬다.
            // writer 가 접수하지 않았다(종료 중). 바뀐 것이 없어 다음 tick 으로 미룬다.
            Err(Blocked::Refused(_)) => return Ok(Some(false)),
            // 확정되지 않았다. 저장 오류와 같이 다룬다.
            Err(blocked @ Blocked::Storage(_)) => return Err(blocked.to_string()),
            // 얻었는지 모른다. 같은 holder 로 다시 얻으면 갱신이라 돌려준 뒤 다음 tick 에 다시 얻는다.
            // 확정되지 않은 점유로 회차를 시작하지 않는다.
            Err(Blocked::Unknown(_)) => {
                self.release_unknown(ws, Holding::Permit, &name, &holder, &task.id);
                return Ok(Some(false));
            }
        };
        if acquired {
            self.held_permits
                .insert(task.id.clone(), (ws, name, holder));
            if let Some(ttl) = ttl_ms {
                self.track_renewal(&task.id, ttl_renewal::Holding::Permit, ttl, now);
            }
        }
        Ok(Some(acquired))
    }

    /// resource 하나 또는 candidates 목록에서 자원을 얻는다. holder 기본값은 task ID다.
    /// elastic을 명시해야 후보를 자동 추가하며 생략하면 고정 목록만 사용한다.
    /// Block 충돌은 Some(false), Fail 충돌과 설정·저장소 오류는 Err다. 획득한 자원은 held_leases에 기록한다.
    pub(super) fn try_acquire_lease(&mut self, task: &Task) -> Result<Option<bool>, String> {
        let Some(meta) = task.metadata.get("lease").and_then(|v| v.as_object()) else {
            return Ok(None);
        };
        let candidates: Vec<String> = if let Some(arr) = meta.get("candidates") {
            let arr = arr
                .as_array()
                .ok_or_else(|| "lease metadata: 'candidates' must be an array".to_string())?;
            arr.iter()
                .map(|v| {
                    v.as_str().map(str::to_string).ok_or_else(|| {
                        "lease metadata: 'candidates' entries must be strings".to_string()
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            let resource = meta
                .get("resource")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "lease metadata: missing 'resource' or 'candidates'".to_string())?;
            vec![resource.to_string()]
        };
        if candidates.is_empty() {
            return Err("lease metadata: 'candidates' must be non-empty".to_string());
        }
        let holder = meta
            .get("holder")
            .and_then(|v| v.as_str())
            .unwrap_or(task.id.as_str());
        let ttl_ms = meta.get("ttl_ms").and_then(|v| v.as_u64());
        let mode = match meta.get("mode").and_then(|v| v.as_str()) {
            Some("fail") => LeaseMode::Fail,
            // 점유가 부족하면 다음 tick에서 다시 시도하도록 기본 모드는 Block이다.
            None | Some("block") => LeaseMode::Block,
            Some(other) => {
                return Err(format!(
                    "lease metadata: invalid mode '{other}' (expected 'fail'|'block')"
                ));
            }
        };
        let elastic: Option<ElasticSpec> = match meta.get("elastic") {
            None => None,
            Some(v) => {
                let obj = v
                    .as_object()
                    .ok_or_else(|| "lease metadata: 'elastic' must be an object".to_string())?;
                let max_candidates = match obj.get("max_candidates") {
                    None => None,
                    Some(v) => Some(v.as_u64().ok_or_else(|| {
                        "lease metadata: 'elastic.max_candidates' must be a non-negative integer"
                            .to_string()
                    })? as u32),
                };
                let overflow_prefix = match obj.get("overflow_prefix") {
                    None => None,
                    Some(v) => Some(
                        v.as_str()
                            .ok_or_else(|| {
                                "lease metadata: 'elastic.overflow_prefix' must be a string"
                                    .to_string()
                            })?
                            .to_string(),
                    ),
                };
                Some(ElasticSpec {
                    max_candidates,
                    overflow_prefix,
                })
            }
        };
        let holder = holder.to_string();
        let ws = task.workspace_id;
        let now = self.holding_clock.now_ms();
        let (pool, h) = (candidates.clone(), holder.clone());
        let result = self.ctx.store(move |mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            match store.acquire_any(ws, &pool, &h, ttl_ms, mode, elastic.as_ref(), now) {
                Ok(o) => Ok((o.acquired, o.resource)),
                Err(AgentError::LeaseConflict { resource, holder }) => {
                    Err(format!("lease conflict: '{resource}' held by '{holder}'"))
                }
                Err(AgentError::LeasePoolExhausted { candidates, holder }) => Err(format!(
                    "lease pool exhausted: none of {candidates:?} available for '{holder}'"
                )),
                Err(e) => Err(e.to_string()),
            }
        });
        let (acquired, resource) = match result {
            Ok(acquired) => acquired?,
            // writer 가 접수하지 않았다(종료 중). 바뀐 것이 없어 다음 tick 으로 미룬다.
            Err(Blocked::Refused(_)) => return Ok(Some(false)),
            // 확정되지 않았다. 저장 오류와 같이 다룬다.
            Err(blocked @ Blocked::Storage(_)) => return Err(blocked.to_string()),
            // 어느 후보를 얻었는지 모른다. 후보마다 같은 holder 의 점유를 돌려준 뒤 다음 tick 에 다시 얻는다.
            Err(Blocked::Unknown(_)) => {
                for resource in &candidates {
                    self.release_unknown(ws, Holding::Lease, resource, &holder, &task.id);
                }
                return Ok(Some(false));
            }
        };
        if acquired {
            let resource = resource.expect("acquire_any: acquired=true implies resource");
            self.held_leases
                .insert(task.id.clone(), (ws, resource, holder));
            if let Some(ttl) = ttl_ms {
                self.track_renewal(&task.id, ttl_renewal::Holding::Lease, ttl, now);
            }
        }
        Ok(Some(acquired))
    }

    pub(super) fn release_lease(&mut self, task_id: &TaskId) {
        self.ttl_renewals
            .remove(&(task_id.clone(), ttl_renewal::Holding::Lease));
        let Some((ws, resource, holder)) = self.held_leases.remove(task_id) else {
            return;
        };
        let (r, h) = (resource.clone(), holder.clone());
        let res: Result<(), String> = self.ctx.with_memory(move |mem| {
            let mut store = LeaseStore::new(mem, HOST_OWNER);
            store
                .release(ws, &r, &h)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            tracing::warn!("lease release failed for task {task_id} ({resource}/{holder}): {e}");
        }
    }

    pub(super) fn release_resources(&mut self, task_id: &TaskId) {
        // permit뿐 아니라 lease·저장된 handle도 정리한다. 프로세스는 이미 끝났다(release_permit).
        self.run_procs.remove(task_id);
        self.late.forget(task_id);
        self.forget_renewals(task_id);
        if let Some((ws, name, holder)) = self.held_permits.remove(task_id) {
            let (n, h) = (name.clone(), holder.clone());
            let res: Result<(), String> = self.ctx.with_memory(move |mem| {
                let mut store = SemaphoreStore::new(mem, HOST_OWNER);
                store
                    .release(ws, &n, &h)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
            if let Err(e) = res {
                tracing::warn!(
                    "semaphore release failed for task {task_id} ({name}/{holder}): {e}"
                );
            }
        }
        self.release_lease(task_id);
        if let Some(ws) = self.held_turns.remove(task_id) {
            self.ctx.agent_turns.release(ws, task_id);
        }
        self.evict_handle(task_id);
    }
}

/// 얻었는지 모르는 점유의 종류.
#[derive(Clone, Copy)]
enum Holding {
    Permit,
    Lease,
}

impl HostExecutor {
    /// 결과 불명으로 끝난 획득을 되돌린다. 같은 holder 의 점유만 돌려주므로 얻지 못했으면 바뀌는 것이
    /// 없다. 돌려주지 못하면 로그를 남긴다. 다음 획득이 같은 holder 로 갱신하고 회차가 끝날 때 돌려준다.
    fn release_unknown(&self, ws: u32, kind: Holding, name: &str, holder: &str, task_id: &TaskId) {
        let (n, h) = (name.to_string(), holder.to_string());
        let released = self.ctx.with_memory(move |mem| -> Result<(), String> {
            match kind {
                Holding::Permit => SemaphoreStore::new(mem, HOST_OWNER)
                    .release(ws, &n, &h)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Holding::Lease => LeaseStore::new(mem, HOST_OWNER)
                    .release(ws, &n, &h)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            }
        });
        if let Err(e) = released {
            tracing::warn!(
                "agent task {task_id}: holding '{name}' with an unknown acquire not released: {e}"
            );
        }
    }
}
