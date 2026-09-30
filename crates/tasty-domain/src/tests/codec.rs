//! codec: 모든 이벤트의 왕복, 모르는 tag·version 거절, 분할 비율 비트 보존, snapshot 왕복.

use std::collections::BTreeSet;

use tasty_event_store::OpaquePayload;

use super::common::{scenario, scenario_batches};
use crate::{
    CodecError, DomainEvent, EVENT_SCHEMA_VERSION, JournalModel, MODEL_VERSION, Ratio,
    decode_event, decode_snapshot, encode_event, encode_snapshot, evolve,
};

#[test]
fn every_event_round_trips_and_the_tag_list_is_complete() {
    let events: Vec<DomainEvent> = scenario().into_iter().flatten().collect();
    let mut seen = BTreeSet::new();
    for event in &events {
        let payload = encode_event(event).expect("encode");
        assert_eq!(payload.type_tag, event.type_tag());
        assert_eq!(payload.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(&decode_event(&payload).expect("decode"), event);
        seen.insert(event.type_tag());
    }
    let known: BTreeSet<&str> = DomainEvent::TAGS.iter().copied().collect();
    assert_eq!(known.len(), DomainEvent::TAGS.len(), "duplicate tag");
    assert_eq!(seen, known, "scenario must cover every known tag");
}

#[test]
fn unknown_tag_is_rejected() {
    let payload = OpaquePayload {
        type_tag: "window.created".to_owned(),
        schema_version: EVENT_SCHEMA_VERSION,
        bytes: b"{}".to_vec(),
    };
    assert!(matches!(
        decode_event(&payload),
        Err(CodecError::UnknownTag(tag)) if tag == "window.created"
    ));
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let event = DomainEvent::TabClosed {
        id: crate::TabId(1),
    };
    let mut payload = encode_event(&event).expect("encode");
    payload.schema_version = EVENT_SCHEMA_VERSION + 1;
    assert!(matches!(
        decode_event(&payload),
        Err(CodecError::UnsupportedVersion { version, .. }) if version == EVENT_SCHEMA_VERSION + 1
    ));
}

#[test]
fn body_that_disagrees_with_the_tag_is_rejected() {
    let event = DomainEvent::TabClosed {
        id: crate::TabId(1),
    };
    let mut payload = encode_event(&event).expect("encode");
    payload.type_tag = "pane.closed".to_owned();
    assert!(matches!(
        decode_event(&payload),
        Err(CodecError::TagMismatch {
            body: "tab.closed",
            ..
        })
    ));
    payload.bytes = b"not json".to_vec();
    assert!(matches!(
        decode_event(&payload),
        Err(CodecError::Body { .. })
    ));
}

#[test]
fn split_ratio_keeps_its_bits() {
    let values = [
        0.1f32 + 0.2,
        f32::MIN_POSITIVE / 3.0,
        f32::from_bits(0x7fc0_1234),
        -0.0,
        1.0 / 3.0,
    ];
    for value in values {
        let event = DomainEvent::PaneSplit {
            target: crate::PaneId(1),
            pane: crate::PaneId(2),
            split: super::common::split(
                crate::SplitDirection::Vertical,
                value,
                crate::Placement::After,
            ),
        };
        let decoded = decode_event(&encode_event(&event).expect("encode")).expect("decode");
        let DomainEvent::PaneSplit { split, .. } = decoded else {
            panic!("wrong variant");
        };
        assert_eq!(split.ratio.to_f32().to_bits(), value.to_bits());
        assert_eq!(split.ratio, Ratio::from_f32(value));
    }
}

#[test]
fn snapshot_round_trips_at_every_batch() {
    let mut model = JournalModel::default();
    for batch in scenario_batches() {
        evolve(&mut model, &batch).expect("evolve");
        let bytes = encode_snapshot(&model).expect("encode");
        let decoded = decode_snapshot(MODEL_VERSION, &bytes).expect("decode");
        assert_eq!(decoded, model);
        assert_eq!(encode_snapshot(&decoded).expect("re-encode"), bytes);
    }
}

#[test]
fn snapshot_with_another_model_version_is_rejected() {
    let bytes = encode_snapshot(&JournalModel::default()).expect("encode");
    assert!(matches!(
        decode_snapshot(MODEL_VERSION + 1, &bytes),
        Err(CodecError::UnsupportedModelVersion(v)) if v == MODEL_VERSION + 1
    ));
    assert!(matches!(
        decode_snapshot(MODEL_VERSION, b"[]"),
        Err(CodecError::Snapshot(_))
    ));
}
