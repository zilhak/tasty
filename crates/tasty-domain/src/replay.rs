//! journal에서 모델 재구성과 snapshot 저장.

use tasty_event_store::{EventStore, NewSnapshot, SnapshotId, StoreError, WriterEpoch};

use crate::codec::{CodecError, MODEL_VERSION, decode_snapshot, encode_snapshot};
use crate::evolve::{EvolveError, evolve};
use crate::model::JournalModel;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Codec(#[from] CodecError),

    #[error(transparent)]
    Evolve(#[from] EvolveError),

    #[error("snapshot {snapshot_id} is at batch {stored:?}, but its model says {model:?}")]
    SnapshotCutMismatch {
        snapshot_id: SnapshotId,
        stored: Option<u64>,
        model: Option<u64>,
    },

    #[error("the model has no applied batch to snapshot")]
    NothingApplied,
}

/// 가장 최근의 검증된 snapshot과 그 뒤 batch로 모델을 만든다. snapshot이 없으면 전체 로그를 쓴다.
/// snapshot 내용을 해석하지 못하면 다른 snapshot으로 넘어가지 않고 오류로 중단한다.
pub fn load(store: &EventStore) -> Result<JournalModel, ReplayError> {
    let replay = store.snapshot_and_tail(MODEL_VERSION)?;
    let mut model = match replay.snapshot {
        Some(snapshot) => {
            let model = decode_snapshot(snapshot.model_version, &snapshot.bytes)?;
            if model.applied.batch != snapshot.cut.last_batch {
                return Err(ReplayError::SnapshotCutMismatch {
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
        evolve(&mut model, batch)?;
    }
    Ok(model)
}

/// 전체 로그만으로 모델을 만든다. snapshot+tail 결과와 대조할 때 쓴다.
pub fn full_replay(store: &EventStore) -> Result<JournalModel, ReplayError> {
    let mut model = JournalModel::default();
    for batch in store.read_batches_after(None, usize::MAX)? {
        evolve(&mut model, &batch)?;
    }
    Ok(model)
}

/// 모델을 마지막 적용 batch 위치의 snapshot으로 저장한다.
pub fn save_snapshot(
    store: &mut EventStore,
    epoch: WriterEpoch,
    model: &JournalModel,
) -> Result<SnapshotId, ReplayError> {
    let batch_id = model.applied.batch.ok_or(ReplayError::NothingApplied)?;
    let referenced_payloads = model
        .surfaces
        .values()
        .filter_map(|s| s.data)
        .map(|d| tasty_event_store::PayloadRef(d.0))
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
