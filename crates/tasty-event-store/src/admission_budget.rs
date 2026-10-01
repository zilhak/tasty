//! Soft active-storage admission budget, not a filesystem quota on accepted obligations.
use crate::{EventStore, StoreError, StoreResult, WriterEpoch};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct AdmissionBudget {
    /// Planning envelope, including operational headroom. Not SQLite max_page_count.
    pub nominal_bytes: u64,
    pub completion_headroom_bytes: u64,
    /// Credit held by each unresolved new command; also its cumulative prepared-payload limit.
    pub command_credit_bytes: u64,
    pub max_pending_effects: u64,
}
impl Default for AdmissionBudget {
    fn default() -> Self {
        Self {
            nominal_bytes: 1024 * 1024 * 1024,
            completion_headroom_bytes: 128 * 1024 * 1024,
            command_credit_bytes: 64 * 1024 * 1024,
            max_pending_effects: 4096,
        }
    }
}
impl AdmissionBudget {
    pub fn admission_ceiling(self) -> u64 {
        self.nominal_bytes
            .saturating_sub(self.completion_headroom_bytes)
    }
    pub(crate) fn validate(self) -> StoreResult<()> {
        if self.max_pending_effects == 0
            || self.command_credit_bytes == 0
            || self.completion_headroom_bytes >= self.nominal_bytes
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
    /// Check only a new command, after durable-key lookup. Existing accepted commands, effects,
    /// cleanup, GC and their duplicate replies must not enter this admission gate.
    /// Caller retains one credit until resolution/cancellation/error and passes all outstanding
    /// credits here. Pinned preparation bytes remain charged by SQLite after credit release.
    pub fn ensure_new_admission(
        &mut self,
        epoch: WriterEpoch,
        outstanding_credits: u64,
    ) -> StoreResult<AdmissionUsage> {
        drop(self.write_tx(epoch)?); // Validate the active fenced writer before maintenance/admission.
        let mut usage = self.admission_usage()?;
        if !self.fits(usage, outstanding_credits) {
            // The store cannot see in-process read leases. Payload GC belongs to the product
            // checkpoint that synchronizes reader pins under its lease lock, never this gate.
            self.try_reclaim_admission_wal()?;
            usage = self.admission_usage()?;
        }
        if !self.fits(usage, outstanding_credits) {
            return Err(StoreError::AdmissionCapacity {
                used: usage.charged_bytes(),
                reserved: outstanding_credits,
                requested: self.admission_budget.command_credit_bytes,
                ceiling: self.admission_budget.admission_ceiling(),
            });
        }
        Ok(usage)
    }
    fn fits(&self, usage: AdmissionUsage, reserved: u64) -> bool {
        usage
            .charged_bytes()
            .checked_add(reserved)
            .and_then(|n| n.checked_add(self.admission_budget.command_credit_bytes))
            .is_some_and(|total| total <= self.admission_budget.admission_ceiling())
    }
    fn try_reclaim_admission_wal(&self) -> StoreResult<()> {
        // PASSIVE does not shrink physical WAL bytes. TRUNCATE may do so when readers permit it;
        // a zero busy timeout means reader pressure is a refusal, never a wait for a reader.
        let timeout: u64 = self
            .conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        self.conn.busy_timeout(Duration::ZERO)?;
        let attempt = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            });
        self.conn.busy_timeout(Duration::from_millis(timeout))?;
        match attempt {
            Ok(_) => Ok(()), // Busy result retains WAL bytes and therefore retains admission pressure.
            Err(rusqlite::Error::SqliteFailure(error, _))
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
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
