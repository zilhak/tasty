//! Opt-in encoded HTTP transport; capability activation belongs to the host.
use crate::{
    html_transfer::TransferId,
    stream::{MAX_FRAME_LEN, StreamFrame, StreamTag},
};
use serde::{Deserialize, Serialize};
mod validation;
pub use validation::decode_chunk;

pub const CAPABILITY: &str = "ipc.stream.html-http-v1";
pub const HTTPS_CAPABILITY: &str = "ipc.stream.html-https-v1";
pub use crate::html_transfer::{AUTO_BYTES, CHUNK_BYTES, DOCUMENT_BYTES};
pub const HEADER_BYTES: usize = 65_536;
pub const HEADER_COUNT: usize = 256;
pub const URL_BYTES: usize = 16_384;
pub const METHOD_BYTES: usize = 32;
pub const REASON_BYTES: usize = 512;
pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResponseId {
    pub request: TransferId,
    pub source_generation: u64,
    pub serial: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyIdentity {
    pub bytes: u64,
    pub sha256: [u8; 32],
}
impl BodyIdentity {
    pub fn checked_len(&self) -> Result<usize> {
        usize::try_from(self.bytes).map_err(|_| "body length exceeds address space")
    }
}
// Intentionally no Debug: names as well as values can contain sensitive metadata.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseHead {
    pub id: ResponseId,
    pub logical_url: String,
    pub status: u16,
    pub headers: Vec<Header>,
    pub encoded_length: Option<u64>,
    pub has_body: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    Closed,
    Stale,
    Capacity,
    InProgress,
    AlreadyUsed,
    DuplicateMismatch,
    Invalid,
    Quota,
    Network,
    Timeout,
    Cancelled,
    UnsupportedScheme,
    UnsupportedUpgrade,
    UnsupportedStreaming,
    UnsupportedEncoding,
    Worker,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum HttpRequest {
    #[serde(rename = "html_http_begin")]
    Begin {
        id: TransferId,
        source_generation: u64,
        method: String,
        logical_url: String,
        headers: Vec<Header>,
        upload: BodyIdentity,
        allow_remote_content: bool,
    },
    #[serde(rename = "html_http_upload")]
    Upload {
        id: TransferId,
        source_generation: u64,
        offset: u64,
        data_b64: String,
    },
    #[serde(rename = "html_http_commit")]
    Commit {
        id: TransferId,
        source_generation: u64,
    },
    #[serde(rename = "html_http_credit")]
    Credit {
        response: ResponseId,
        offset: u64,
        max_bytes: u32,
        allow_large: bool,
    },
    #[serde(rename = "html_http_cancel")]
    Cancel {
        id: TransferId,
        source_generation: u64,
    },
    #[serde(rename = "html_http_reuse")]
    Reuse { response: ResponseId },
    #[serde(rename = "html_http_release")]
    Release { response: ResponseId },
    #[serde(rename = "html_http_stop")]
    Stop {
        entry: TransferId,
        source_generation: u64,
    },
    #[serde(rename = "html_http_close")]
    Close {
        entry: TransferId,
        source_generation: u64,
    },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum HttpReply {
    #[serde(rename = "html_http_upload_ready")]
    UploadReady {
        id: TransferId,
        source_generation: u64,
        offset: u64,
        max_bytes: u32,
    },
    #[serde(rename = "html_http_head")]
    Head { head: ResponseHead },
    #[serde(rename = "html_http_allowance_required")]
    AllowanceRequired {
        response: ResponseId,
        encoded_at_least: u64,
        known_length: Option<u64>,
    },
    #[serde(rename = "html_http_chunk")]
    Chunk {
        response: ResponseId,
        offset: u64,
        data_b64: String,
    },
    #[serde(rename = "html_http_complete")]
    Complete {
        response: ResponseId,
        encoded: BodyIdentity,
        decoded: BodyIdentity,
    },
    #[serde(rename = "html_http_failure")]
    Failure {
        id: TransferId,
        source_generation: u64,
        response: Option<ResponseId>,
        code: FailureCode,
        reason: String,
    },
    #[serde(rename = "html_http_cancelled")]
    Cancelled {
        id: TransferId,
        source_generation: u64,
        response: Option<ResponseId>,
    },
}
fn frame<T: Serialize>(value: &T) -> Result<StreamFrame> {
    let payload = serde_json::to_vec(value).map_err(|_| "HTTP frame serialization")?;
    if payload.len() > MAX_FRAME_LEN as usize {
        return Err("HTTP frame payload limit");
    }
    Ok(StreamFrame::new(StreamTag::Control, payload))
}
impl HttpRequest {
    pub fn id(&self) -> TransferId {
        match self {
            Self::Begin { id, .. }
            | Self::Upload { id, .. }
            | Self::Commit { id, .. }
            | Self::Cancel { id, .. } => *id,
            Self::Credit { response, .. }
            | Self::Reuse { response }
            | Self::Release { response } => response.request,
            Self::Stop { entry, .. } | Self::Close { entry, .. } => *entry,
        }
    }
    pub fn source_generation(&self) -> u64 {
        match self {
            Self::Begin {
                source_generation, ..
            }
            | Self::Upload {
                source_generation, ..
            }
            | Self::Commit {
                source_generation, ..
            }
            | Self::Cancel {
                source_generation, ..
            }
            | Self::Stop {
                source_generation, ..
            }
            | Self::Close {
                source_generation, ..
            } => *source_generation,
            Self::Credit { response, .. }
            | Self::Reuse { response }
            | Self::Release { response } => response.source_generation,
        }
    }
    pub fn frame(&self) -> Result<StreamFrame> {
        self.validate()?;
        frame(self)
    }
}
impl HttpReply {
    pub fn id(&self) -> TransferId {
        match self {
            Self::UploadReady { id, .. }
            | Self::Failure { id, .. }
            | Self::Cancelled { id, .. } => *id,
            Self::Head { head } => head.id.request,
            Self::AllowanceRequired { response, .. }
            | Self::Chunk { response, .. }
            | Self::Complete { response, .. } => response.request,
        }
    }
    pub fn frame(&self) -> Result<StreamFrame> {
        self.validate()?;
        frame(self)
    }
}
#[cfg(test)]
mod tests;
