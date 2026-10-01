//! 도메인 이벤트·snapshot 본문의 codec. 모르는 type tag나 schema version은 추측하지 않고 오류로 중단한다.
//!
//! 저장 봉투·저장 형식 버전·migration은 이벤트 저장소가 정한다. 여기서는 봉투에 담을 tag·version·
//! 본문 바이트만 만든다.

use crate::event::DomainEvent;
use crate::streams::StructureModels;

/// 이 빌드가 쓰고 읽는 이벤트 schema version.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// 4 adds immutable undo records and separates live payload pins from completed operation history.
pub const MODEL_VERSION: u32 = 4;

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

/// 저장 봉투에 담을 이벤트 본문.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedEvent {
    pub type_tag: String,
    pub schema_version: u32,
    pub bytes: Vec<u8>,
}

pub fn encode_event(event: &DomainEvent) -> Result<EncodedEvent, CodecError> {
    Ok(EncodedEvent {
        type_tag: event.type_tag().to_owned(),
        schema_version: EVENT_SCHEMA_VERSION,
        bytes: serde_json::to_vec(event).map_err(CodecError::Encode)?,
    })
}

pub fn decode_event(
    type_tag: &str,
    schema_version: u32,
    bytes: &[u8],
) -> Result<DomainEvent, CodecError> {
    let tag = type_tag;
    if !DomainEvent::TAGS.contains(&tag) {
        return Err(CodecError::UnknownTag(tag.to_owned()));
    }
    if schema_version != EVENT_SCHEMA_VERSION {
        return Err(CodecError::UnsupportedVersion {
            tag: tag.to_owned(),
            version: schema_version,
        });
    }
    let event: DomainEvent = serde_json::from_slice(bytes).map_err(|source| CodecError::Body {
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

pub fn encode_snapshot(models: &StructureModels) -> Result<Vec<u8>, CodecError> {
    serde_json::to_vec(models).map_err(CodecError::Encode)
}

pub fn decode_snapshot(model_version: u32, bytes: &[u8]) -> Result<StructureModels, CodecError> {
    if !matches!(model_version, 2 | MODEL_VERSION) {
        return Err(CodecError::UnsupportedModelVersion(model_version));
    }
    serde_json::from_slice(bytes).map_err(CodecError::Snapshot)
}
