//! CoreState 쪽 구조 digest 입력. CoreState를 직접 읽은 정규 표현과, capture → importer → journal
//! 모델을 거친 정규 표현을 만든다. 정규 표현과 비교 범위는 [`crate::runtime::shadow_digest`]가 정한다.
//!
//! 시험 전용이며 제품 경로에 연결하지 않는다.

mod tests;

use tasty_domain::{DataRef, JournalModel};
use tasty_event_store::{EventStore, PayloadRef, WriterEpoch};

use super::surface_data::SurfaceData;
use super::{ImportError, ImportOutcome, ScrollbackSource, import_slot};
use crate::core::CoreState;
use crate::core::layout_persistence::LayoutSlotId;
use crate::core::layout_persistence::schema::SavedLayout;
use crate::runtime::journal;
use crate::runtime::shadow_digest::{CanonData, Canonical, ResolveData, fnv_hex, sorted_value};

pub(super) fn core_canonical(engine: &CoreState) -> Canonical {
    crate::runtime::shadow_digest::live::core_canonical(engine)
}

/// capture가 이 kind를 그대로 저장하는지. terminal은 registry를 보지 않고 저장하며, 그 밖의 surface는
/// capture와 같은 registry 조회(철회된 정의 포함)로 정한다. 대기 plugin은 등록 여부와 관계없이
/// kind를 유지하지만 kind 문자열만으로는 구별하지 못해 registry 판정을 따른다.
pub(super) fn capture_keeps_kind(engine: &CoreState, kind: &str) -> bool {
    kind == super::TERMINAL_KIND || engine.runtime.surface_registry.get(kind).is_some()
}

/// CoreState를 슬롯으로 capture해 journal의 그 슬롯 엔진 stream에 가져온 뒤 그 모델을 읽는다.
/// capture는 scrollback 저장 ID를 새로 정할 수 있어 engine을 바꿀 수 있다.
pub(super) fn capture_and_import(
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    store: &mut EventStore,
    epoch: WriterEpoch,
    slot: LayoutSlotId,
    scrollback: &dyn ScrollbackSource,
) -> Result<(ImportOutcome, JournalModel), ImportError> {
    let layout = SavedLayout::capture(
        engine,
        0,
        &crate::model::StructurePresentationSnapshot::default(),
    );
    let json = serde_json::to_string(&layout).map_err(ImportError::Mapping)?;
    let outcome = import_slot(store, epoch, slot, &json, scrollback)?;
    let model = journal::load(store, &journal::engine_stream(slot))?;
    Ok((outcome, model))
}

/// surface 저장 자료를 kind 소유자의 형식으로 해석한다. scrollback은 길이와 해시로 줄인다.
pub(super) struct DecodedData<'a>(pub(super) &'a EventStore);

impl ResolveData for DecodedData<'_> {
    fn resolve(&self, data: DataRef) -> CanonData {
        let bytes = match self.0.read_payload(PayloadRef(data.0)) {
            Ok(bytes) => bytes,
            Err(error) => return CanonData::Unreadable(error.to_string()),
        };
        match SurfaceData::decode(&bytes) {
            Ok(SurfaceData::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
                scrollback,
            }) => CanonData::Decoded(serde_json::json!({
                "terminal": {
                    "cwd": cwd,
                    "restore_command": restore_command,
                    "scrollback_ref": scrollback_ref,
                    "scrollback": scrollback.map(|b| serde_json::json!({
                        "len": b.len(),
                        "hash": fnv_hex(&b),
                    })),
                }
            })),
            Ok(SurfaceData::Generic { data }) => {
                CanonData::Decoded(serde_json::json!({ "generic": sorted_value(&data) }))
            }
            Err(error) => CanonData::Unreadable(error.to_string()),
        }
    }
}
