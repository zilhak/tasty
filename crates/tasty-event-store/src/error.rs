//! 저장소 오류. 거절 사유를 호출자가 구분할 수 있도록 종류별로 나눈다.

use crate::effect::EffectState;
use crate::types::{CommandStatus, ExpectedRevision, WriterEpoch};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{kind} logical write bytes {bytes} exceed limit {limit}")]
    WriteSizeExceeded {
        kind: &'static str,
        bytes: usize,
        limit: usize,
    },
    #[error("pending effect capacity: existing={pending}, new={new_effects}, limit={limit}")]
    PendingEffectCapacity {
        pending: u64,
        new_effects: u64,
        limit: u64,
    },

    #[error("invalid journal admission budget")]
    AdmissionBudgetInvalid,
    #[error("journal admission usage: {0}")]
    AdmissionUsageIo(std::io::Error),
    #[error(
        "journal admission capacity: used={used}, reserved={reserved}, requested={requested}, ceiling={ceiling}"
    )]
    AdmissionCapacity {
        used: u64,
        reserved: u64,
        requested: u64,
        ceiling: u64,
    },
    #[error(
        "admission prepared payload capacity: used={used}, requested={requested}, limit={limit}"
    )]
    AdmissionPayloadCapacity {
        used: u64,
        requested: u64,
        limit: u64,
    },

    #[error("projection scope: {0}")]
    ProjectionScope(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("journal schema version {found} is newer than supported {supported}")]
    SchemaTooNew { found: u32, supported: u32 },

    #[error("durability unavailable: {pragma} is {effective}, required {required}")]
    Durability {
        pragma: &'static str,
        required: &'static str,
        effective: String,
    },

    #[error("file is a non-empty SQLite database without a journal version table")]
    NotAJournal,

    #[error("journal id mismatch: requested {requested}, stored {stored}")]
    JournalMismatch { requested: String, stored: String },

    #[error("journal is archived and accepts no writes")]
    JournalArchived,

    #[error("another store holds the journal writer lock")]
    WriterLocked,

    #[error("journal writer lock is unavailable: {0}")]
    WriterLockUnavailable(std::io::Error),

    #[error("this store does not hold the journal writer lock")]
    NotWriter,

    #[error("writer epoch {presented:?} is fenced by current {current:?}")]
    Fenced {
        presented: WriterEpoch,
        current: WriterEpoch,
    },

    #[error("stream {stream}: expected {expected:?}, actual {actual:?}")]
    RevisionConflict {
        stream: String,
        expected: ExpectedRevision,
        actual: Option<u64>,
    },

    #[error("append to stream {0} has no events")]
    EmptyAppend(String),

    #[error("stream {0} appears twice in one batch")]
    DuplicateStream(String),

    #[error("command key ({caller_scope}, {idempotency_key}) is bound to a different request")]
    KeyConflict {
        caller_scope: String,
        idempotency_key: String,
    },

    #[error("unknown command {0}")]
    UnknownCommand(String),

    #[error("command {command_id} already finished as {status:?}")]
    CommandFinished {
        command_id: String,
        status: CommandStatus,
    },

    #[error("command {command_id} cannot move back from {from:?} to {to:?}")]
    CommandRegression {
        command_id: String,
        from: CommandStatus,
        to: CommandStatus,
    },

    #[error("reservation for {0} asks for zero ids")]
    EmptyReservation(String),

    #[error("id space {kind} is exhausted: {count} ids from {next} would pass {max_id}")]
    IdSpaceExhausted {
        kind: String,
        next: u64,
        count: u64,
        max_id: u64,
    },

    #[error("unknown effect {0}")]
    UnknownEffect(String),

    #[error("effect {effect_id}: expected state {expected:?}, actual {actual:?}")]
    EffectStateMismatch {
        effect_id: String,
        expected: EffectState,
        actual: EffectState,
    },

    #[error("effect {effect_id}: transition {from:?} -> {to:?} is not allowed")]
    InvalidEffectTransition {
        effect_id: String,
        from: EffectState,
        to: EffectState,
    },

    #[error("effect {effect_id}: generation {presented} does not match {current}")]
    StaleGeneration {
        effect_id: String,
        presented: u64,
        current: u64,
    },

    #[error("effect {effect_id}: attempt {presented:?} does not match {current}")]
    StaleAttempt {
        effect_id: String,
        presented: Option<u32>,
        current: u32,
    },

    #[error("effect {effect_id}: initial state {state:?} must be Pending or Deferred")]
    InvalidInitialEffectState {
        effect_id: String,
        state: EffectState,
    },

    #[error("effect {0}: cancelling an uncertain effect needs evidence that it did not run")]
    EvidenceRequired(String),

    #[error("effect {0}: transition to Running needs an activation claim")]
    ClaimRequired(String),

    #[error("effect {effect_id}: activation claim is held by effect {holder}")]
    ClaimHeld { effect_id: String, holder: String },

    #[error("payload {0} does not exist")]
    PayloadMissing(u64),

    #[error("payload {payload} has {size} bytes, exceeding read limit {limit}")]
    PayloadTooLarge {
        payload: u64,
        size: u64,
        limit: usize,
    },

    #[error("payload {0} failed checksum verification")]
    PayloadCorrupt(u64),

    #[error("batch {0} does not exist")]
    UnknownBatch(u64),

    #[error(
        "history at or before batch {retained_after_batch} was compacted; resynchronize from a snapshot"
    )]
    ResyncRequired { retained_after_batch: u64 },

    #[error("checkpoint for {consumer_id} would move back from batch {current} to {requested}")]
    CheckpointRegression {
        consumer_id: String,
        current: u64,
        requested: u64,
    },

    #[error("consumer {0} keeps projection rows; move its position with commit_projection")]
    CheckpointOwnedByProjection(String),

    #[error("projection key {0} appears twice in one write")]
    ProjectionKeyConflict(String),

    #[error("restore alias {0} is frozen for another input or has no verifiable input identity")]
    RestoreAliasConflict(String),

    #[error("restore manifest {0} would replace a newer saved View")]
    ManifestRegression(String),

    #[error("stored value is out of range: {0}")]
    Corrupt(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
