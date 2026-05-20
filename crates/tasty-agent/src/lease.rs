//! Lease primitive — 임의 resource 에 대한 협조적 점유 마커.
//!
//! 본 단계(Phase 5.3)는 **poll-based + 협조적 (advisory)** 모델이다. OS 락이
//! 아니므로 lease 를 무시한 채 resource 를 만지는 행위 자체는 막지 못한다.
//! 다중 에이전트가 자발적으로 점유 상태를 조회하고, 충돌이면 우회하거나
//! 재시도하기로 약속할 때 의미가 있다.
//!
//! 영속: `tasty.agent.lease.<resource>` (workspace scope).
//!
//! 모드:
//! - `fail` (기본): 점유 충돌 시 `AgentError::LeaseConflict` 로 즉시 실패.
//! - `block`: 점유 충돌 시 `acquired=false` 로 반환 (호출자가 다시 시도).
//!
//! TTL:
//! - `ttl_ms` 가 있으면 `expires_at = acquired_at + ttl_ms`. 만료된 lease 는
//!   다음 `acquire` 호출 시점에 lazy 하게 evict (다른 holder 가 점유 가능).
//!
//! 점유 규칙:
//! - 같은 holder 가 다시 `acquire` 하면 idempotent 갱신 (acquired_at /
//!   expires_at 만 갱신, 점유는 유지).
//! - `release` 는 점유 holder 만 가능; 다른 holder 가 `release` 호출하면 no-op.

use serde::{Deserialize, Serialize};
use tasty_core::model::WorkspaceId;
use tasty_memory::{ListOpts, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::{AgentError, Result};

pub const LEASE_KEY_PREFIX: &str = "tasty.agent.lease.";

/// resource 문자열을 memory 키 허용 문자(`[a-z0-9._-]`)만 쓰는 stable 토큰으로
/// 변환. 디코딩이 필요 없으므로(원본은 JSON value 에 같이 저장됨) 안전한 단방향
/// 인코딩이면 충분. `_` 는 escape sentinel 이라 `__` 로 더블링하고, 그 외
/// 비허용 바이트는 `_<hex>` 로 치환한다 — 두 형식이 충돌하지 않음.
fn lease_key(resource: &str) -> String {
    let mut out = String::from(LEASE_KEY_PREFIX);
    for b in resource.bytes() {
        if b == b'_' {
            out.push_str("__");
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' {
            out.push(b as char);
        } else {
            out.push('_');
            out.push_str(&format!("{:02x}", b));
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMode {
    /// 충돌 시 즉시 `LeaseConflict` 에러 반환.
    Fail,
    /// 충돌 시 `acquired=false` 로 반환 — 호출자가 polling.
    Block,
}

impl Default for LeaseMode {
    fn default() -> Self {
        Self::Fail
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub workspace_id: WorkspaceId,
    pub resource: String,
    pub holder: String,
    pub acquired_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl Lease {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        match self.expires_at {
            Some(t) => now_ms >= t,
            None => false,
        }
    }
}

/// `acquire` 응답. `mode=block` 에서 점유 충돌 시 `acquired=false` 와 함께
/// 현재 점유 중인 lease 가 들어 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquireOutcome {
    pub acquired: bool,
    pub lease: Lease,
}

/// `release` 응답.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseOutcome {
    /// 호출 holder 가 실제로 점유 중이었는지 (즉, 본 호출로 release 가 일어났는지).
    pub released: bool,
    /// release 직전의 lease (또는 다른 holder 점유 시 그 lease). resource 자체가
    /// 점유 중이 아니었으면 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<Lease>,
}

pub struct LeaseStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
}

impl<'a> LeaseStore<'a> {
    pub fn new(mem: &'a mut MemoryStore, owner: impl Into<String>) -> Self {
        Self {
            mem,
            owner: owner.into(),
        }
    }

    fn put(&mut self, lease: &Lease) -> Result<()> {
        let scope = Scope::Workspace(lease.workspace_id);
        let value = MemoryValue::Json(serde_json::to_value(lease)?);
        self.mem.put(
            &self.owner,
            &scope,
            &lease_key(&lease.resource),
            &value,
            &PutOpts::default(),
        )?;
        Ok(())
    }

    fn delete_key(&mut self, workspace_id: WorkspaceId, resource: &str) -> Result<()> {
        let scope = Scope::Workspace(workspace_id);
        self.mem
            .delete(&self.owner, &scope, &lease_key(resource), None)?;
        Ok(())
    }

    pub fn get(&self, workspace_id: WorkspaceId, resource: &str) -> Result<Option<Lease>> {
        let scope = Scope::Workspace(workspace_id);
        let entry = self.mem.get(&scope, &lease_key(resource))?;
        match entry {
            Some(e) => match e.value {
                MemoryValue::Json(v) => Ok(Some(serde_json::from_value(v)?)),
                _ => Err(AgentError::InvalidArgument(format!(
                    "lease entry is not json: {resource}"
                ))),
            },
            None => Ok(None),
        }
    }

    /// 워크스페이스의 모든 lease. `now_ms` 가 있으면 만료된 lease 는 evict 한 뒤
    /// 결과에서도 제외한다 (영속도 제거).
    pub fn list(&mut self, workspace_id: WorkspaceId, now_ms: Option<u64>) -> Result<Vec<Lease>> {
        let scope = Scope::Workspace(workspace_id);
        let opts = ListOpts {
            prefix: Some(LEASE_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&scope, &opts)?;
        let mut alive: Vec<Lease> = Vec::with_capacity(entries.len());
        let mut expired: Vec<String> = Vec::new();
        for e in entries {
            if let MemoryValue::Json(v) = e.value {
                let lease: Lease = serde_json::from_value(v)?;
                if let Some(now) = now_ms {
                    if lease.is_expired(now) {
                        expired.push(lease.resource.clone());
                        continue;
                    }
                }
                alive.push(lease);
            }
        }
        for r in expired {
            self.delete_key(workspace_id, &r)?;
        }
        Ok(alive)
    }

    /// resource 에 대해 lease 점유 시도. 같은 holder 면 idempotent 갱신 (TTL 도 재설정).
    /// 다른 holder 가 점유 중이고 만료 안 됐다면 mode 에 따라 분기한다.
    pub fn acquire(
        &mut self,
        workspace_id: WorkspaceId,
        resource: &str,
        holder: &str,
        ttl_ms: Option<u64>,
        mode: LeaseMode,
        now_ms: u64,
    ) -> Result<AcquireOutcome> {
        if resource.is_empty() {
            return Err(AgentError::InvalidArgument(
                "lease.resource must be non-empty".into(),
            ));
        }
        if holder.is_empty() {
            return Err(AgentError::InvalidArgument(
                "lease.holder must be non-empty".into(),
            ));
        }

        let existing = self.get(workspace_id, resource)?;
        if let Some(cur) = existing {
            if cur.holder == holder {
                // idempotent 재acquire — TTL 갱신.
                let lease = Lease {
                    workspace_id,
                    resource: resource.to_string(),
                    holder: holder.to_string(),
                    acquired_at: now_ms,
                    expires_at: ttl_ms.map(|t| now_ms.saturating_add(t)),
                };
                self.put(&lease)?;
                return Ok(AcquireOutcome {
                    acquired: true,
                    lease,
                });
            }
            if !cur.is_expired(now_ms) {
                return match mode {
                    LeaseMode::Fail => Err(AgentError::LeaseConflict {
                        resource: resource.to_string(),
                        holder: cur.holder.clone(),
                    }),
                    LeaseMode::Block => Ok(AcquireOutcome {
                        acquired: false,
                        lease: cur,
                    }),
                };
            }
            // 만료 — evict 후 새로 점유.
        }

        let lease = Lease {
            workspace_id,
            resource: resource.to_string(),
            holder: holder.to_string(),
            acquired_at: now_ms,
            expires_at: ttl_ms.map(|t| now_ms.saturating_add(t)),
        };
        self.put(&lease)?;
        Ok(AcquireOutcome {
            acquired: true,
            lease,
        })
    }

    /// 점유 holder 만 release 가능. 다른 holder 가 호출하면 no-op (현 점유는 유지).
    pub fn release(
        &mut self,
        workspace_id: WorkspaceId,
        resource: &str,
        holder: &str,
    ) -> Result<ReleaseOutcome> {
        let existing = self.get(workspace_id, resource)?;
        let Some(cur) = existing else {
            return Ok(ReleaseOutcome {
                released: false,
                lease: None,
            });
        };
        if cur.holder != holder {
            return Ok(ReleaseOutcome {
                released: false,
                lease: Some(cur),
            });
        }
        self.delete_key(workspace_id, resource)?;
        Ok(ReleaseOutcome {
            released: true,
            lease: Some(cur),
        })
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
    fn acquire_then_conflict_fail_mode() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        let r = store
            .acquire(1, "file:/a", "h1", None, LeaseMode::Fail, 1000)
            .unwrap();
        assert!(r.acquired);
        let err = store
            .acquire(1, "file:/a", "h2", None, LeaseMode::Fail, 1100)
            .unwrap_err();
        match err {
            AgentError::LeaseConflict { resource, holder } => {
                assert_eq!(resource, "file:/a");
                assert_eq!(holder, "h1");
            }
            _ => panic!("expected LeaseConflict, got {err:?}"),
        }
    }

    #[test]
    fn acquire_conflict_block_mode_returns_acquired_false() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        store
            .acquire(1, "file:/a", "h1", None, LeaseMode::Fail, 1000)
            .unwrap();
        let r = store
            .acquire(1, "file:/a", "h2", None, LeaseMode::Block, 1100)
            .unwrap();
        assert!(!r.acquired);
        assert_eq!(r.lease.holder, "h1");
    }

    #[test]
    fn same_holder_reacquire_is_idempotent_and_renews_ttl() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        let r1 = store
            .acquire(1, "file:/a", "h1", Some(500), LeaseMode::Fail, 1000)
            .unwrap();
        assert_eq!(r1.lease.expires_at, Some(1500));
        let r2 = store
            .acquire(1, "file:/a", "h1", Some(800), LeaseMode::Fail, 1200)
            .unwrap();
        assert!(r2.acquired);
        assert_eq!(r2.lease.acquired_at, 1200);
        assert_eq!(r2.lease.expires_at, Some(2000));
    }

    #[test]
    fn expired_lease_can_be_taken_by_other_holder() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        store
            .acquire(1, "file:/a", "h1", Some(500), LeaseMode::Fail, 1000)
            .unwrap();
        // 만료 시점 이후 다른 holder 가 acquire.
        let r = store
            .acquire(1, "file:/a", "h2", None, LeaseMode::Fail, 2000)
            .unwrap();
        assert!(r.acquired);
        assert_eq!(r.lease.holder, "h2");
    }

    #[test]
    fn release_only_by_holder() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        store
            .acquire(1, "file:/a", "h1", None, LeaseMode::Fail, 1000)
            .unwrap();
        let r = store.release(1, "file:/a", "h2").unwrap();
        assert!(!r.released);
        // 여전히 h1 점유.
        let cur = store.get(1, "file:/a").unwrap().unwrap();
        assert_eq!(cur.holder, "h1");

        let r = store.release(1, "file:/a", "h1").unwrap();
        assert!(r.released);
        assert!(store.get(1, "file:/a").unwrap().is_none());
    }

    #[test]
    fn release_unheld_resource_is_noop() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        let r = store.release(1, "file:/none", "h1").unwrap();
        assert!(!r.released);
        assert!(r.lease.is_none());
    }

    #[test]
    fn list_evicts_expired_when_now_supplied() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        store
            .acquire(1, "file:/a", "h1", Some(500), LeaseMode::Fail, 1000)
            .unwrap();
        store
            .acquire(1, "file:/b", "h2", None, LeaseMode::Fail, 1000)
            .unwrap();
        let alive = store.list(1, Some(2000)).unwrap();
        let resources: Vec<_> = alive.iter().map(|l| l.resource.clone()).collect();
        assert_eq!(resources, vec!["file:/b".to_string()]);
        // /a 가 영속에서도 제거되었는지 확인.
        assert!(store.get(1, "file:/a").unwrap().is_none());
    }

    #[test]
    fn empty_resource_or_holder_rejected() {
        let (_td, mut mem) = fresh();
        let mut store = LeaseStore::new(&mut mem, "_host");
        let err = store
            .acquire(1, "", "h1", None, LeaseMode::Fail, 1000)
            .unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
        let err = store
            .acquire(1, "file:/a", "", None, LeaseMode::Fail, 1000)
            .unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }
}
