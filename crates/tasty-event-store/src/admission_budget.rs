//! Bounded unresolved preparation, independent of cumulative journal storage.
use crate::{EventStore, StoreError, StoreResult, WriterEpoch};

#[derive(Debug, Clone, Copy)]
pub struct AdmissionBudget {
    /// Aggregate credit for unresolved preparations; persisted history never consumes it.
    pub max_pending_preparation_bytes: u64,
    /// Credit held by each unresolved new command; also its cumulative prepared-payload limit.
    pub command_credit_bytes: u64,
    pub max_pending_effects: u64,
}
impl Default for AdmissionBudget {
    fn default() -> Self {
        Self {
            max_pending_preparation_bytes: 14 * 64 * 1024 * 1024,
            command_credit_bytes: 64 * 1024 * 1024,
            max_pending_effects: 4096,
        }
    }
}
impl AdmissionBudget {
    pub fn admission_ceiling(self) -> u64 {
        self.max_pending_preparation_bytes
    }
    pub(crate) fn validate(self) -> StoreResult<()> {
        if self.max_pending_effects == 0
            || self.command_credit_bytes == 0
            || self.command_credit_bytes > self.admission_ceiling()
        {
            return Err(StoreError::AdmissionBudgetInvalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct AdmissionUsage {
    pub database_file_bytes: u64,
    pub wal_file_bytes: u64,
    pub reusable_database_bytes: u64,
    pub active_database_bytes: u64,
}
impl AdmissionUsage {
    pub fn charged_bytes(self) -> u64 {
        self.active_database_bytes
            .saturating_add(self.wal_file_bytes)
    }
}
impl EventStore {
    pub fn admission_budget(&self) -> AdmissionBudget {
        self.admission_budget
    }
    pub fn admission_usage(&self) -> StoreResult<AdmissionUsage> {
        let page_size: u64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let pages: u64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let free: u64 = self
            .conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
        let mut wal = self.database_path.as_os_str().to_os_string();
        wal.push("-wal");
        Ok(AdmissionUsage {
            database_file_bytes: file_bytes(&self.database_path)?,
            wal_file_bytes: file_bytes(std::path::Path::new(&wal))?,
            reusable_database_bytes: free.saturating_mul(page_size),
            active_database_bytes: pages.saturating_sub(free).saturating_mul(page_size),
        })
    }
    /// Fence a new request and bound only unresolved preparation credits. Cumulative DB/WAL
    /// bytes are diagnostic metadata, never grounds to refuse terminal startup or a new tab.
    /// Actual SQLite and filesystem errors remain errors, including failures reading usage.
    pub fn ensure_new_admission(
        &mut self,
        epoch: WriterEpoch,
        outstanding_credits: u64,
    ) -> StoreResult<AdmissionUsage> {
        drop(self.write_tx(epoch)?);
        if outstanding_credits
            .checked_add(self.admission_budget.command_credit_bytes)
            .is_none_or(|total| total > self.admission_budget.admission_ceiling())
        {
            return Err(StoreError::AdmissionCapacity {
                used: 0,
                reserved: outstanding_credits,
                requested: self.admission_budget.command_credit_bytes,
                ceiling: self.admission_budget.admission_ceiling(),
            });
        }
        self.admission_usage()
    }
}
fn file_bytes(path: &std::path::Path) -> StoreResult<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(StoreError::AdmissionUsageIo(error)),
    }
}

/// Only creating new obligations consumes this budget. Existing transitions and reconciliation
/// must remain possible even when an older database already exceeds the configured limit.
pub(crate) fn require_effect_capacity(
    conn: &rusqlite::Connection,
    new_effects: usize,
    limit: u64,
) -> StoreResult<()> {
    if new_effects == 0 {
        return Ok(());
    }
    let pending: u64 = conn.query_row(
        "SELECT COUNT(*) FROM effects WHERE state IN ('pending','deferred','running','uncertain')",
        [],
        |row| row.get(0),
    )?;
    if pending
        .checked_add(new_effects as u64)
        .is_none_or(|total| total > limit)
    {
        return Err(StoreError::PendingEffectCapacity {
            pending,
            new_effects: new_effects as u64,
            limit,
        });
    }
    Ok(())
}
