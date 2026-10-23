//! Opt-in HTML transport on Control frames. This does not advertise a native viewer.
use crate::stream::{MAX_FRAME_LEN, StreamFrame, StreamTag};
use serde::{Deserialize, Serialize};

pub const CAPABILITY: &str = "ipc.stream.html-transfer-v1";
pub const VIEWER_CAPABILITY: &str = "ipc.stream.html-entry-v1";
pub const AUTO_BYTES: u64 = 100_000;
pub const DOCUMENT_BYTES: u64 = 2_000_000;
pub const CHUNK_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HtmlSource {
    File {
        path: String,
        #[serde(default)]
        url: String,
    },
    Http {
        url: String,
    },
    Inline,
    Unsupported,
}

/// Echoed unchanged. Local bindings are never inferred from a numeric remote surface ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId {
    pub epoch: u64,
    pub surface_id: u32,
    pub binding: u64,
    pub document: u64,
    pub request: u64,
    pub transfer: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlMetadata {
    pub source: HtmlSource,
    pub source_generation: u64,
    pub root: Option<String>,
    pub cwd: Option<String>,
    pub size: u64,
    pub regular: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HtmlRequest {
    HtmlCapabilities {
        capabilities: Vec<String>,
    },
    HtmlEntryRequest {
        id: TransferId,
    },
    /// Paths are decoded, relative to the original authorized entry root (never a client path).
    HtmlResourceRequest {
        id: TransferId,
        entry: TransferId,
        source_generation: u64,
        relative: String,
    },
    /// One reply window per credit. Offset is the exact next raw byte offset.
    HtmlRead {
        id: TransferId,
        offset: u64,
        allow: bool,
    },
    HtmlCancel {
        id: TransferId,
    },
}
impl HtmlRequest {
    pub fn id(&self) -> Option<TransferId> {
        match self {
            Self::HtmlCapabilities { .. } => None,
            Self::HtmlEntryRequest { id }
            | Self::HtmlResourceRequest { id, .. }
            | Self::HtmlRead { id, .. }
            | Self::HtmlCancel { id } => Some(*id),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HtmlReply {
    HtmlMetadata {
        id: TransferId,
        metadata: HtmlMetadata,
    },
    HtmlChunk {
        id: TransferId,
        offset: u64,
        data_b64: String,
    },
    HtmlComplete {
        id: TransferId,
        size: u64,
    },
    HtmlFailure {
        id: TransferId,
        reason: String,
    },
    HtmlCancelled {
        id: TransferId,
    },
}
impl HtmlReply {
    pub fn id(&self) -> TransferId {
        match self {
            Self::HtmlMetadata { id, .. }
            | Self::HtmlChunk { id, .. }
            | Self::HtmlComplete { id, .. }
            | Self::HtmlFailure { id, .. }
            | Self::HtmlCancelled { id } => *id,
        }
    }
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::HtmlMetadata { metadata, .. } => {
                let source = match &metadata.source {
                    HtmlSource::File { path, url } => {
                        path.capacity().saturating_add(url.capacity())
                    }
                    HtmlSource::Http { url } => url.capacity(),
                    _ => 0,
                };
                source
                    .saturating_add(metadata.root.as_ref().map_or(0, String::capacity))
                    .saturating_add(metadata.cwd.as_ref().map_or(0, String::capacity))
            }
            Self::HtmlChunk { data_b64, .. } => data_b64.capacity(),
            Self::HtmlFailure { reason, .. } => reason.capacity(),
            _ => 0,
        }
    }
    /// Bound the serialized envelope, including escaped metadata and base64, before enqueue.
    pub fn frame(&self) -> Result<StreamFrame, String> {
        let payload = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if payload.len() > MAX_FRAME_LEN as usize {
            return Err("HTML frame exceeds payload limit".into());
        }
        Ok(StreamFrame::new(StreamTag::Control, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_limit_includes_metadata_and_base64() {
        let id = TransferId {
            epoch: u64::MAX,
            surface_id: u32::MAX,
            binding: u64::MAX,
            document: u64::MAX,
            request: u64::MAX,
            transfer: u64::MAX,
        };
        let reply = HtmlReply::HtmlChunk {
            id,
            offset: u64::MAX,
            data_b64: "A".repeat(CHUNK_BYTES.div_ceil(3) * 4),
        };
        assert!(reply.frame().unwrap().payload.len() < MAX_FRAME_LEN as usize);
        assert!(
            HtmlReply::HtmlFailure {
                id,
                reason: "\"".repeat(MAX_FRAME_LEN as usize)
            }
            .frame()
            .is_err()
        );
        assert_eq!(crate::stream::STREAM_PROTO, 1);
    }
}
