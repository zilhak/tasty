//! 엔진별 구조 stream. journal 하나에 엔진마다 stream이 하나 있고, 이름은 [`STRUCTURE_STREAM_PREFIX`]로 시작한다.
//!
//! batch 하나는 여러 엔진 stream을 함께 바꿀 수 있다. [`evolve_streams`]는 batch를 모든 엔진 모델에
//! 한꺼번에 적용하며, 한 stream이라도 실패하면 어느 모델도 바꾸지 않는다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{DomainBatch, RecordedEvent};
use crate::evolve::{EvolveError, check_batch, evolve_owned};
use crate::ids::BatchId;
use crate::model::JournalModel;

/// 엔진 구조 stream 이름의 접두. 이 접두가 없는 stream은 구조 이벤트로 해석하지 않는다.
pub const STRUCTURE_STREAM_PREFIX: &str = "structure:";

pub fn is_structure_stream(stream: &str) -> bool {
    stream.starts_with(STRUCTURE_STREAM_PREFIX)
}

/// 한 batch cut까지 적용한 모든 엔진 구조 모델. stream 이름이 키다.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StructureModels {
    /// 마지막으로 적용한 batch. 구조 이벤트가 없는 batch도 포함한다.
    pub batch: Option<BatchId>,
    pub streams: BTreeMap<String, JournalModel>,
}

impl StructureModels {
    /// 엔진 stream의 모델. 아직 이벤트가 없는 stream이면 빈 모델이다.
    pub fn stream(&self, stream: &str) -> JournalModel {
        self.streams.get(stream).cloned().unwrap_or_default()
    }
}

/// 확정 batch 하나의 엔진 stream별 도메인 입력. 이벤트가 없는 stream은 담지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamBatch {
    pub batch_id: BatchId,
    pub streams: BTreeMap<String, Vec<RecordedEvent>>,
}

/// batch를 모든 엔진 모델에 적용한다. 이벤트가 없는 모델도 적용 위치를 옮긴다.
/// 처음 나온 stream은 빈 모델에서 시작한다. 오류이면 호출 전 그대로다.
pub fn evolve_streams(
    models: &mut StructureModels,
    batch: &StreamBatch,
) -> Result<(), EvolveError> {
    if let Some(last) = models.batch
        && batch.batch_id <= last
    {
        return Err(EvolveError::StaleBatch {
            last,
            got: batch.batch_id,
        });
    }
    for model in models.streams.values() {
        check_batch(model, batch.batch_id)?;
    }
    let mut staged = BTreeMap::new();
    for (stream, events) in &batch.streams {
        if events.is_empty() && models.streams.contains_key(stream) {
            continue;
        }
        let next = evolve_owned(
            models.stream(stream),
            &DomainBatch {
                batch_id: batch.batch_id,
                events: events.clone(),
            },
        )?;
        staged.insert(stream.clone(), next);
    }
    // No fallible operation follows: publish all candidates and advance unchanged cuts together.
    models.streams.extend(staged);
    for model in models.streams.values_mut() {
        model.applied.batch = Some(batch.batch_id);
    }
    models.batch = Some(batch.batch_id);
    Ok(())
}
