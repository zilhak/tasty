//! Semaphore primitive — N개 permit 까지 동시 점유 허용.
//!
//! 본 단계(Phase 5.2)에서는 **poll-based acquire** 만 제공한다. 가용 permit 이
//! 없으면 `acquire` 가 `acquired=false` 로 즉시 응답하고, 호출자가 다시 시도한다.
//! 실제 blocking + queue/fairness 는 scheduler 도입 후 추가.
//!
//! 영속: `tasty.agent.semaphore.<name>` (workspace scope).
//!
//! 점유 규칙:
//! - 같은 `holder` 가 이미 점유 중이면 acquire 는 idempotent 하게 성공 응답
//!   (재시도 안전). 동일 holder 가 permits 를 두 개 잡고 싶다면 다른 holder id
//!   를 써야 한다.
//! - `release` 는 holders 에서 해당 id 제거 + permits_available 1 회복. holder
//!   가 점유 중이 아니면 no-op (idempotent).

use serde::{Deserialize, Serialize};
use tasty_memory::{ListOpts, MemoryStore, MemoryValue, PutOpts, Scope};
use tasty_utils::id::WorkspaceId;

use crate::{AgentError, Result};

pub const SEMAPHORE_KEY_PREFIX: &str = "tasty.agent.semaphore.";

fn semaphore_key(name: &str) -> String {
    format!("{SEMAPHORE_KEY_PREFIX}{name}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Semaphore {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub permits_total: u32,
    pub permits_available: u32,
    pub holders: Vec<String>,
    pub created_at: u64,
}

/// `acquire` 응답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquireOutcome {
    pub acquired: bool,
    pub semaphore: Semaphore,
}

/// `release` 응답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseOutcome {
    /// 실제로 release 되어 permits 가 회복되었는지 (점유 중이 아니었으면 false).
    pub released: bool,
    pub semaphore: Semaphore,
}

pub struct SemaphoreStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
}

impl<'a> SemaphoreStore<'a> {
    pub fn new(mem: &'a mut MemoryStore, owner: impl Into<String>) -> Self {
        Self {
            mem,
            owner: owner.into(),
        }
    }

    fn put(&mut self, s: &Semaphore) -> Result<()> {
        let scope = Scope::Workspace(s.workspace_id);
        let value = MemoryValue::Json(serde_json::to_value(s)?);
        self.mem.put(
            &self.owner,
            &scope,
            &semaphore_key(&s.name),
            &value,
            &PutOpts::default(),
        )?;
        Ok(())
    }

    pub fn get(&self, workspace_id: WorkspaceId, name: &str) -> Result<Option<Semaphore>> {
        let scope = Scope::Workspace(workspace_id);
        let entry = self.mem.get(&scope, &semaphore_key(name))?;
        match entry {
            Some(e) => match e.value {
                MemoryValue::Json(v) => Ok(Some(serde_json::from_value(v)?)),
                _ => Err(AgentError::InvalidArgument(format!(
                    "semaphore entry is not json: {name}"
                ))),
            },
            None => Ok(None),
        }
    }

    pub fn list(&self, workspace_id: WorkspaceId) -> Result<Vec<Semaphore>> {
        let scope = Scope::Workspace(workspace_id);
        let opts = ListOpts {
            prefix: Some(SEMAPHORE_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&scope, &opts)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            if let MemoryValue::Json(v) = e.value {
                out.push(serde_json::from_value(v)?);
            }
        }
        Ok(out)
    }

    pub fn create(
        &mut self,
        workspace_id: WorkspaceId,
        name: impl Into<String>,
        permits: u32,
        now_ms: u64,
    ) -> Result<Semaphore> {
        let name = name.into();
        if permits == 0 {
            return Err(AgentError::InvalidArgument(
                "semaphore.permits must be >= 1".into(),
            ));
        }
        if let Some(existing) = self.get(workspace_id, &name)? {
            return Err(AgentError::InvalidArgument(format!(
                "semaphore '{name}' already exists (permits_total={})",
                existing.permits_total
            )));
        }
        let s = Semaphore {
            workspace_id,
            name,
            permits_total: permits,
            permits_available: permits,
            holders: Vec::new(),
            created_at: now_ms,
        };
        self.put(&s)?;
        Ok(s)
    }

    /// permit 1개 획득 시도. 이미 동일 holder 가 점유 중이면 idempotent 성공.
    pub fn acquire(
        &mut self,
        workspace_id: WorkspaceId,
        name: &str,
        holder: &str,
    ) -> Result<AcquireOutcome> {
        if holder.is_empty() {
            return Err(AgentError::InvalidArgument(
                "holder must be non-empty".into(),
            ));
        }
        let mut s = self
            .get(workspace_id, name)?
            .ok_or_else(|| AgentError::InvalidArgument(format!("semaphore not found: {name}")))?;
        if s.holders.iter().any(|h| h == holder) {
            return Ok(AcquireOutcome {
                acquired: true,
                semaphore: s,
            });
        }
        if s.permits_available == 0 {
            return Ok(AcquireOutcome {
                acquired: false,
                semaphore: s,
            });
        }
        s.permits_available -= 1;
        s.holders.push(holder.to_string());
        self.put(&s)?;
        Ok(AcquireOutcome {
            acquired: true,
            semaphore: s,
        })
    }

    /// permit 반환. holder 가 점유 중이 아니면 no-op.
    pub fn release(
        &mut self,
        workspace_id: WorkspaceId,
        name: &str,
        holder: &str,
    ) -> Result<ReleaseOutcome> {
        let mut s = self
            .get(workspace_id, name)?
            .ok_or_else(|| AgentError::InvalidArgument(format!("semaphore not found: {name}")))?;
        let before = s.holders.len();
        s.holders.retain(|h| h != holder);
        let released = s.holders.len() != before;
        if released {
            s.permits_available = s.permits_available.saturating_add(1).min(s.permits_total);
        }
        if released {
            self.put(&s)?;
        }
        Ok(ReleaseOutcome {
            released,
            semaphore: s,
        })
    }

    pub fn delete(&mut self, workspace_id: WorkspaceId, name: &str) -> Result<()> {
        let scope = Scope::Workspace(workspace_id);
        self.mem
            .delete(&self.owner, &scope, &semaphore_key(name), None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_memory::MemoryStore;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, MemoryStore) {
        let td = tempfile::tempdir().unwrap();
        let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
        (td, mem)
    }

    #[test]
    fn create_then_two_acquires_third_fails() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        store.create(1, "s1", 2, 1000).unwrap();
        let r1 = store.acquire(1, "s1", "h1").unwrap();
        assert!(r1.acquired);
        assert_eq!(r1.semaphore.permits_available, 1);
        let r2 = store.acquire(1, "s1", "h2").unwrap();
        assert!(r2.acquired);
        assert_eq!(r2.semaphore.permits_available, 0);
        let r3 = store.acquire(1, "s1", "h3").unwrap();
        assert!(!r3.acquired);
        assert_eq!(r3.semaphore.permits_available, 0);
        assert_eq!(r3.semaphore.holders.len(), 2);
    }

    #[test]
    fn acquire_idempotent_for_same_holder() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        store.create(1, "s1", 1, 1000).unwrap();
        let r1 = store.acquire(1, "s1", "h1").unwrap();
        assert!(r1.acquired);
        let r2 = store.acquire(1, "s1", "h1").unwrap();
        assert!(r2.acquired);
        assert_eq!(r2.semaphore.permits_available, 0);
        assert_eq!(r2.semaphore.holders, vec!["h1"]);
    }

    #[test]
    fn release_restores_permit() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        store.create(1, "s1", 2, 1000).unwrap();
        store.acquire(1, "s1", "h1").unwrap();
        let rel = store.release(1, "s1", "h1").unwrap();
        assert!(rel.released);
        assert_eq!(rel.semaphore.permits_available, 2);
        assert!(rel.semaphore.holders.is_empty());
    }

    #[test]
    fn release_unknown_holder_is_noop() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        store.create(1, "s1", 2, 1000).unwrap();
        store.acquire(1, "s1", "h1").unwrap();
        let rel = store.release(1, "s1", "ghost").unwrap();
        assert!(!rel.released);
        assert_eq!(rel.semaphore.permits_available, 1);
    }

    #[test]
    fn permits_zero_rejected() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        let err = store.create(1, "s", 0, 1000).unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }

    #[test]
    fn duplicate_create_rejected() {
        let (_td, mut mem) = fresh();
        let mut store = SemaphoreStore::new(&mut mem, "_host");
        store.create(1, "s1", 2, 1000).unwrap();
        let err = store.create(1, "s1", 3, 1001).unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }
}
