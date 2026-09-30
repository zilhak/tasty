#![forbid(unsafe_code)]

//! 인스턴스별 로컬 SQLite event journal.
//!
//! 한 journal 파일에 stream(엔진)별 revision, 여러 stream을 묶는 원자 batch, 재시도 키로 찾는
//! 명령 기록, effect 의무와 시도 기록, domain snapshot·consumer checkpoint, 불변 payload를 둔다.
//! 이벤트·명령·effect는 [`EventStore::commit`] 한 transaction으로 확정되며, 실패하면 아무것도
//! 남지 않는다.
//!
//! 이 크레이트는 도메인 타입을 모른다. 이벤트·effect·snapshot 내용은 type tag·schema version·
//! 바이트로만 저장하며 해석은 호출자의 codec이 맡는다.
//!
//! 보장 범위:
//! - 한 journal 안의 원자성·순서만 보장한다. memory.db 등 다른 저장소와의 원자성은 없다.
//! - WAL과 `synchronous=FULL`이 실제로 적용되지 않으면 열지 않는다. 저장 장치가 sync를
//!   지키는지는 이 크레이트가 확인하지 않는다.
//! - 쓰기는 독점 writer 잠금을 가진 저장소만 하며 세대 검사를 쓰기마다 한다. 잠금은 다른 저장소·
//!   프로세스를, 세대는 같은 저장소 안의 늦은 writer·worker를 막는다. 읽기용으로 여는 것은 막지 않는다.

mod command;
mod commit;
mod effect;
mod error;
mod payload;
mod read;
mod schema;
mod snapshot;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use commit::{CommitOutcome, CommitRequest, event_holder};
pub use effect::{
    ActivationClaim, AttemptRecord, EffectRecord, EffectState, EffectTransition, NewEffect,
};
pub use error::{StoreError, StoreResult};
pub use schema::SCHEMA_VERSION;
pub use snapshot::{
    DomainSnapshot, NewSnapshot, RejectedSnapshot, Replay, SnapshotId, snapshot_holder,
};
pub use store::EventStore;
pub use types::{
    BatchCut, BatchId, CommandKey, CommandLookup, CommandRecord, CommandStatus, CommandUpdate,
    ExpectedRevision, JournalCut, NewCommand, NewEvent, OpaquePayload, PayloadRef, Revision,
    StoredBatch, StoredEvent, StreamAppend, StreamId, WriterEpoch,
};
