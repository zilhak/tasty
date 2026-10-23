//! Several logical requests in one fenced physical transaction. Not an independent worker.
use crate::command::find_by_key;
use crate::metrics::WriteTransaction;
use crate::{CommitOutcome, CommitRequest, EventStore, StoreError, StoreResult, WriterEpoch};

/// Private transaction view. Successful appends remain invisible to other connections until finish.
/// Dropping this value rolls back every request. A database error poisons the whole group.
pub struct CommitGroup<'a> {
    tx: WriteTransaction<'a>,
    journal_id: String,
    epoch: WriterEpoch,
    effect_limit: u64,
    logical: u64,
    bytes: usize,
    poisoned: bool,
}

impl EventStore {
    pub fn begin_commit_group(&mut self, epoch: WriterEpoch) -> StoreResult<CommitGroup<'_>> {
        let journal_id = self.journal_id().to_owned();
        let effect_limit = self.admission_budget.max_pending_effects;
        let tx = self.write_tx(epoch)?;
        Ok(CommitGroup {
            tx,
            journal_id,
            epoch,
            effect_limit,
            logical: 0,
            bytes: 0,
            poisoned: false,
        })
    }
}

impl CommitGroup<'_> {
    /// Request failures roll back only that request. SQLite failures require dropping this group.
    pub fn append(&mut self, request: &CommitRequest) -> StoreResult<CommitOutcome> {
        if self.poisoned {
            return Err(StoreError::GroupAborted);
        }
        if request.writer_epoch != self.epoch {
            return Err(StoreError::Fenced {
                presented: request.writer_epoch,
                current: self.epoch,
            });
        }
        if let Err(error) = self.tx.execute_batch("SAVEPOINT journal_request") {
            self.poisoned = true;
            return Err(error.into());
        }
        let result = self.append_in(request);
        match result {
            Ok((outcome, bytes)) => {
                if let Err(error) = self.tx.execute_batch("RELEASE journal_request") {
                    self.poisoned = true;
                    return Err(error.into());
                }
                if matches!(outcome, CommitOutcome::Committed { .. }) {
                    self.logical += 1;
                    self.bytes = self.bytes.saturating_add(bytes);
                }
                Ok(outcome)
            }
            Err(error) => {
                self.poisoned = matches!(error, StoreError::Sqlite(_)) || self.tx.is_autocommit();
                if !self.tx.is_autocommit() {
                    if let Err(rollback) = self
                        .tx
                        .execute_batch("ROLLBACK TO journal_request; RELEASE journal_request")
                    {
                        self.poisoned = true;
                        return Err(rollback.into());
                    }
                }
                Err(error)
            }
        }
    }

    fn append_in(&self, request: &CommitRequest) -> StoreResult<(CommitOutcome, usize)> {
        if let Some(new) = &request.command
            && let Some(key) = &new.key
            && let Some(existing) = find_by_key(&self.tx, key)?
        {
            if existing.request_digest != new.request_digest {
                return Err(StoreError::KeyConflict {
                    caller_scope: key.caller_scope.clone(),
                    idempotency_key: key.idempotency_key.clone(),
                });
            }
            return Ok((CommitOutcome::Duplicate(existing), 0));
        }
        crate::admission_budget::require_effect_capacity(
            &self.tx,
            request.effects.len(),
            self.effect_limit,
        )?;
        let (batch, bytes) = crate::commit::write_all(&self.tx, request, &self.journal_id)?;
        Ok((CommitOutcome::Committed { batch }, bytes))
    }

    pub fn read_batch(&self, batch: crate::BatchId) -> StoreResult<crate::StoredBatch> {
        crate::read::load_batch(&self.tx, batch)
    }

    pub fn lookup_command(
        &self,
        key: &crate::CommandKey,
        digest: &[u8],
    ) -> StoreResult<crate::CommandLookup> {
        Ok(match find_by_key(&self.tx, key)? {
            None => crate::CommandLookup::Miss,
            Some(record) if record.request_digest == digest => crate::CommandLookup::Hit(record),
            Some(record) => crate::CommandLookup::DigestMismatch(record),
        })
    }

    /// Whether this valid logical request fits the current internal group byte bound.
    /// A false value asks the caller to finish this group and start another, never to reject it.
    pub fn fits(&self, request: &CommitRequest) -> StoreResult<bool> {
        let bytes = crate::write_limits::commit(request)?.len();
        Ok(self.logical == 0
            || self.bytes.saturating_add(bytes) <= crate::write_limits::MAX_WRITE_BYTES)
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn writer_epoch(&self) -> WriterEpoch {
        self.epoch
    }

    pub fn command(&self, id: &str) -> StoreResult<Option<crate::CommandRecord>> {
        crate::command::find_by_id(&self.tx, id)
    }

    pub fn effect(&self, id: &str) -> StoreResult<Option<crate::EffectRecord>> {
        crate::effect::find_by_id(&self.tx, id)
    }

    pub fn finish(self) -> StoreResult<()> {
        if self.poisoned {
            return Err(StoreError::GroupAborted);
        }
        if self.logical == 0 {
            return Ok(());
        }
        self.tx.structural(self.logical).commit()?;
        Ok(())
    }
}
