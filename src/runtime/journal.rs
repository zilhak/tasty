//! 저장 batch ↔ 도메인 batch 변환과 journal에서의 모델 재구성·snapshot 저장.
//!
//! 저장 봉투(`OpaquePayload`)와 도메인 이벤트 본문 사이의 변환은 여기서만 한다. journal 하나에
//! 엔진마다 구조 stream이 하나 있으며 이름은 [`engine_stream`]이 정한다. 구조 stream이 아닌 이벤트는
//! 해석하지 않고 건너뛰며, batch 위치는 모든 엔진 모델에 그대로 넘긴다.

use std::fmt;

use tasty_core::{
    CodecError, DomainEvent, EvolveError, JournalModel, MODEL_VERSION, RecordedEvent,
    STRUCTURE_STREAM_PREFIX, StreamBatch, StructureModels, decode_event, decode_snapshot,
    encode_event, encode_snapshot, evolve_streams, is_structure_stream,
};
use tasty_event_store::{
    EventStore, NewSnapshot, OpaquePayload, PayloadRef, SnapshotId, StoreError, StoredBatch,
    StreamId, WriterEpoch,
};

/// 엔진의 구조 stream. 엔진의 영속 식별은 그 엔진이 쓰는 레이아웃 슬롯 번호다.
pub(crate) fn engine_stream(slot: u32) -> StreamId {
    StreamId::new(format!("{STRUCTURE_STREAM_PREFIX}slot-{slot}"))
}

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
    #[cfg(test)]
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
            #[cfg(test)]
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

/// 저장 batch에서 엔진 구조 stream 이벤트만 stream별로 해석한다. 모르는 tag·version이면 멈춘다.
pub(crate) fn stream_batch(batch: &StoredBatch) -> Result<StreamBatch, CodecError> {
    let mut out = StreamBatch {
        batch_id: batch.cut.batch_id,
        streams: Default::default(),
    };
    for stored in &batch.events {
        if !is_structure_stream(stored.stream_id.as_str()) {
            continue;
        }
        let payload = &stored.payload;
        out.streams
            .entry(stored.stream_id.as_str().to_owned())
            .or_default()
            .push(RecordedEvent {
                revision: stored.stream_revision,
                event: decode_event(&payload.type_tag, payload.schema_version, &payload.bytes)?,
            });
    }
    Ok(out)
}

/// 저장 batch 하나를 모든 엔진 모델에 적용한다. 해석이나 적용이 실패하면 어느 모델도 바뀌지 않는다.
pub(crate) fn apply_all(
    models: &mut StructureModels,
    batch: &StoredBatch,
) -> Result<(), JournalError> {
    evolve_streams(models, &stream_batch(batch)?)?;
    Ok(())
}

/// 저장 batch 하나를 엔진 stream 하나의 모델에 적용한다. 다른 stream은 해석만 하고 적용하지 않는다.
#[cfg(test)]
pub(crate) fn apply(
    model: &mut JournalModel,
    stream: &StreamId,
    batch: &StoredBatch,
) -> Result<(), JournalError> {
    let mut decoded = stream_batch(batch)?;
    let events = decoded.streams.remove(stream.as_str()).unwrap_or_default();
    tasty_core::evolve(
        model,
        &tasty_core::DomainBatch {
            batch_id: decoded.batch_id,
            events,
        },
    )?;
    Ok(())
}

/// 가장 최근의 검증된 snapshot과 그 뒤 batch로 모든 엔진 모델을 만든다. snapshot이 없으면 전체 로그를 쓴다.
/// snapshot 내용을 해석하지 못하면 다른 snapshot으로 넘어가지 않고 오류로 중단한다.
pub(crate) fn load_all(store: &EventStore) -> Result<StructureModels, JournalError> {
    let replay = store.snapshot_and_tail(MODEL_VERSION)?;
    let mut models = match replay.snapshot {
        Some(snapshot) => {
            let models = decode_snapshot(snapshot.model_version, &snapshot.bytes)?;
            if models.batch != snapshot.cut.last_batch {
                return Err(JournalError::SnapshotCutMismatch {
                    snapshot_id: snapshot.snapshot_id,
                    stored: snapshot.cut.last_batch,
                    model: models.batch,
                });
            }
            models
        }
        None => StructureModels::default(),
    };
    for batch in &replay.tail {
        apply_all(&mut models, batch)?;
    }
    Ok(models)
}

/// 엔진 stream 하나의 모델. 이벤트가 아직 없는 엔진이면 빈 모델이다.
pub(crate) fn load(store: &EventStore, stream: &StreamId) -> Result<JournalModel, JournalError> {
    Ok(load_all(store)?.stream(stream.as_str()))
}

/// 전체 로그만으로 모든 엔진 모델을 만든다. snapshot+tail 결과와 대조할 때 쓴다.
#[cfg(test)]
pub(crate) fn full_replay(store: &EventStore) -> Result<StructureModels, JournalError> {
    let mut models = StructureModels::default();
    for batch in store.read_batches_after(None, usize::MAX)? {
        apply_all(&mut models, &batch)?;
    }
    Ok(models)
}

/// 모든 엔진 모델을 마지막 적용 batch 위치의 snapshot 하나로 저장한다. surface 자료 참조를 함께 pin한다.
#[cfg(test)]
pub(crate) fn save_snapshot(
    store: &mut EventStore,
    epoch: WriterEpoch,
    models: &StructureModels,
) -> Result<SnapshotId, JournalError> {
    let batch_id = models.batch.ok_or(JournalError::NothingApplied)?;
    let referenced_payloads = models
        .streams
        .values()
        .flat_map(JournalModel::data_refs)
        .map(|d| PayloadRef(d.0))
        .collect();
    Ok(store.save_snapshot(
        epoch,
        &NewSnapshot {
            batch_id,
            model_version: MODEL_VERSION,
            bytes: encode_snapshot(models)?,
            referenced_payloads,
        },
    )?)
}
