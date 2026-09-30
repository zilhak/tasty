//! 저장 batch ↔ 도메인 batch 변환과 journal에서의 모델 재구성·snapshot 저장.
//!
//! 저장 봉투(`OpaquePayload`)와 도메인 이벤트 본문 사이의 변환은 여기서만 한다. 구조 stream이
//! 아닌 이벤트는 해석하지 않고 건너뛰며, batch 위치는 도메인 batch로 그대로 넘긴다.

use std::fmt;

use tasty_domain::{
    CodecError, DomainBatch, DomainEvent, EvolveError, JournalModel, MODEL_VERSION, RecordedEvent,
    STRUCTURE_STREAM, decode_event, decode_snapshot, encode_event, encode_snapshot, evolve,
};
use tasty_event_store::{
    EventStore, NewSnapshot, OpaquePayload, PayloadRef, SnapshotId, StoreError, StoredBatch,
    WriterEpoch,
};

#[derive(Debug)]
pub(crate) enum JournalError {
    Store(StoreError),
    Codec(CodecError),
    Evolve(EvolveError),
    /// snapshot 모델의 적용 위치가 저장소가 기록한 snapshot 위치와 다르다.
    SnapshotCutMismatch {
        snapshot_id: SnapshotId,
        stored: Option<u64>,
        model: Option<u64>,
    },
    /// 아직 적용한 batch가 없는 모델은 snapshot으로 저장하지 않는다.
    NothingApplied,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Codec(error) => write!(f, "{error}"),
            Self::Evolve(error) => write!(f, "{error}"),
            Self::SnapshotCutMismatch {
                snapshot_id,
                stored,
                model,
            } => write!(
                f,
                "snapshot {snapshot_id} is at batch {stored:?}, but its model says {model:?}"
            ),
            Self::NothingApplied => f.write_str("the model has no applied batch to snapshot"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<StoreError> for JournalError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<CodecError> for JournalError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<EvolveError> for JournalError {
    fn from(error: EvolveError) -> Self {
        Self::Evolve(error)
    }
}

/// 도메인 이벤트를 저장 봉투에 담는다.
pub(crate) fn to_payload(event: &DomainEvent) -> Result<OpaquePayload, CodecError> {
    let encoded = encode_event(event)?;
    Ok(OpaquePayload {
        type_tag: encoded.type_tag,
        schema_version: encoded.schema_version,
        bytes: encoded.bytes,
    })
}

/// 저장 batch에서 구조 stream 이벤트만 해석한다. 모르는 tag·version이면 멈춘다.
pub(crate) fn domain_batch(batch: &StoredBatch) -> Result<DomainBatch, CodecError> {
    let mut events = Vec::new();
    for stored in &batch.events {
        if stored.stream_id.as_str() != STRUCTURE_STREAM {
            continue;
        }
        let payload = &stored.payload;
        events.push(RecordedEvent {
            revision: stored.stream_revision,
            event: decode_event(&payload.type_tag, payload.schema_version, &payload.bytes)?,
        });
    }
    Ok(DomainBatch {
        batch_id: batch.cut.batch_id,
        events,
    })
}

/// 저장 batch 하나를 모델에 적용한다. 해석이나 적용이 실패하면 모델은 그대로다.
pub(crate) fn apply(model: &mut JournalModel, batch: &StoredBatch) -> Result<(), JournalError> {
    evolve(model, &domain_batch(batch)?)?;
    Ok(())
}

/// 가장 최근의 검증된 snapshot과 그 뒤 batch로 모델을 만든다. snapshot이 없으면 전체 로그를 쓴다.
/// snapshot 내용을 해석하지 못하면 다른 snapshot으로 넘어가지 않고 오류로 중단한다.
pub(crate) fn load(store: &EventStore) -> Result<JournalModel, JournalError> {
    let replay = store.snapshot_and_tail(MODEL_VERSION)?;
    let mut model = match replay.snapshot {
        Some(snapshot) => {
            let model = decode_snapshot(snapshot.model_version, &snapshot.bytes)?;
            if model.applied.batch != snapshot.cut.last_batch {
                return Err(JournalError::SnapshotCutMismatch {
                    snapshot_id: snapshot.snapshot_id,
                    stored: snapshot.cut.last_batch,
                    model: model.applied.batch,
                });
            }
            model
        }
        None => JournalModel::default(),
    };
    for batch in &replay.tail {
        apply(&mut model, batch)?;
    }
    Ok(model)
}

/// 전체 로그만으로 모델을 만든다. snapshot+tail 결과와 대조할 때 쓴다.
pub(crate) fn full_replay(store: &EventStore) -> Result<JournalModel, JournalError> {
    let mut model = JournalModel::default();
    for batch in store.read_batches_after(None, usize::MAX)? {
        apply(&mut model, &batch)?;
    }
    Ok(model)
}

/// 모델을 마지막 적용 batch 위치의 snapshot으로 저장한다. surface 자료 참조를 함께 pin한다.
pub(crate) fn save_snapshot(
    store: &mut EventStore,
    epoch: WriterEpoch,
    model: &JournalModel,
) -> Result<SnapshotId, JournalError> {
    let batch_id = model.applied.batch.ok_or(JournalError::NothingApplied)?;
    let referenced_payloads = model
        .surfaces
        .values()
        .filter_map(|s| s.data)
        .map(|d| PayloadRef(d.0))
        .collect();
    Ok(store.save_snapshot(
        epoch,
        &NewSnapshot {
            batch_id,
            model_version: MODEL_VERSION,
            bytes: encode_snapshot(model)?,
            referenced_payloads,
        },
    )?)
}
