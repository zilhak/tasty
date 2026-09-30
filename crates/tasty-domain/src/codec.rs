//! 이벤트·snapshot codec. 모르는 type tag나 schema version은 추측하지 않고 오류로 중단한다.

use tasty_event_store::OpaquePayload;

use crate::event::DomainEvent;
use crate::model::JournalModel;

/// 이 빌드가 쓰고 읽는 이벤트 schema version.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// 이 빌드가 쓰고 읽는 snapshot model version. 다른 version의 snapshot은 저장소가 건너뛴다.
pub const MODEL_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("unknown event type tag {0}")]
    UnknownTag(String),

    #[error("event {tag}: unsupported schema version {version}")]
    UnsupportedVersion { tag: String, version: u32 },

    #[error("event {tag}: body does not match the type tag: {source}")]
    Body {
        tag: String,
        source: serde_json::Error,
    },

    #[error("event body says {body}, type tag says {tag}")]
    TagMismatch { tag: String, body: &'static str },

    #[error("snapshot model version {0} is not supported")]
    UnsupportedModelVersion(u32),

    #[error("snapshot body is invalid: {0}")]
    Snapshot(serde_json::Error),

    #[error("encoding failed: {0}")]
    Encode(serde_json::Error),
}

pub fn encode_event(event: &DomainEvent) -> Result<OpaquePayload, CodecError> {
    Ok(OpaquePayload {
        type_tag: event.type_tag().to_owned(),
        schema_version: EVENT_SCHEMA_VERSION,
        bytes: serde_json::to_vec(event).map_err(CodecError::Encode)?,
    })
}

pub fn decode_event(payload: &OpaquePayload) -> Result<DomainEvent, CodecError> {
    let tag = payload.type_tag.as_str();
    if !DomainEvent::TAGS.contains(&tag) {
        return Err(CodecError::UnknownTag(tag.to_owned()));
    }
    if payload.schema_version != EVENT_SCHEMA_VERSION {
        return Err(CodecError::UnsupportedVersion {
            tag: tag.to_owned(),
            version: payload.schema_version,
        });
    }
    let event: DomainEvent =
        serde_json::from_slice(&payload.bytes).map_err(|source| CodecError::Body {
            tag: tag.to_owned(),
            source,
        })?;
    if event.type_tag() != tag {
        return Err(CodecError::TagMismatch {
            tag: tag.to_owned(),
            body: event.type_tag(),
        });
    }
    Ok(event)
}

pub fn encode_snapshot(model: &JournalModel) -> Result<Vec<u8>, CodecError> {
    serde_json::to_vec(model).map_err(CodecError::Encode)
}

pub fn decode_snapshot(model_version: u32, bytes: &[u8]) -> Result<JournalModel, CodecError> {
    if model_version != MODEL_VERSION {
        return Err(CodecError::UnsupportedModelVersion(model_version));
    }
    serde_json::from_slice(bytes).map_err(CodecError::Snapshot)
}
