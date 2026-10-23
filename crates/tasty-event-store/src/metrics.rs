//! Successful physical write transactions, counted at SQLite COMMIT rather than API entry.

use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rusqlite::Transaction;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitStatistics {
    /// All successful explicit write transactions since opening this connection, including init.
    pub physical_commits: u64,
    /// Transactions issued by commit/commit_with_restore_checkpoint (not ID/pin/claim writes).
    pub structural_commits: u64,
    pub logical_requests: u64,
    pub failed_commits: u64,
    /// SQLite COMMIT call only, excluding transaction building, decide, queue and publication.
    pub commit_ns: u64,
    pub max_commit_ns: u64,
}

#[derive(Default)]
pub(crate) struct CommitMetrics {
    physical: AtomicU64,
    structural: AtomicU64,
    logical: AtomicU64,
    failed: AtomicU64,
    ns: AtomicU64,
    max_ns: AtomicU64,
}

impl CommitMetrics {
    pub(crate) fn snapshot(&self) -> CommitStatistics {
        CommitStatistics {
            physical_commits: self.physical.load(Ordering::Relaxed),
            structural_commits: self.structural.load(Ordering::Relaxed),
            logical_requests: self.logical.load(Ordering::Relaxed),
            failed_commits: self.failed.load(Ordering::Relaxed),
            commit_ns: self.ns.load(Ordering::Relaxed),
            max_commit_ns: self.max_ns.load(Ordering::Relaxed),
        }
    }
}

/// Keeps every existing fenced writer on the same measurement path. Dropping/rolling back a
/// transaction (including a duplicate hit) does not count as a successful physical commit.
pub(crate) struct WriteTransaction<'a> {
    inner: Transaction<'a>,
    metrics: Arc<CommitMetrics>,
    logical: Option<u64>,
}

impl<'a> WriteTransaction<'a> {
    pub(crate) fn new(inner: Transaction<'a>, metrics: Arc<CommitMetrics>) -> Self {
        Self {
            inner,
            metrics,
            logical: None,
        }
    }

    pub(crate) fn structural(mut self, logical: u64) -> Self {
        self.logical = Some(logical);
        self
    }

    pub(crate) fn commit(self) -> rusqlite::Result<()> {
        let start = Instant::now();
        let result = self.inner.commit();
        let ns = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        if result.is_ok() {
            self.metrics.physical.fetch_add(1, Ordering::Relaxed);
            if let Some(logical) = self.logical {
                self.metrics.structural.fetch_add(1, Ordering::Relaxed);
                self.metrics.logical.fetch_add(logical, Ordering::Relaxed);
            }
            self.metrics.ns.fetch_add(ns, Ordering::Relaxed);
            self.metrics.max_ns.fetch_max(ns, Ordering::Relaxed);
        } else {
            self.metrics.failed.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
}

impl<'a> Deref for WriteTransaction<'a> {
    type Target = Transaction<'a>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
