//! 저장 계약의 값 타입. 도메인 타입을 모르므로 payload는 태그·버전·바이트로만 다룬다.

use std::collections::BTreeMap;
use std::fmt;

/// stream(엔진) 식별자. 저장소는 의미를 해석하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(pub String);

impl StreamId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// stream 안의 확정 이벤트 순번. 첫 이벤트가 1이며 단조 증가한다.
pub type Revision = u64;

/// 확정 batch 번호. 한 journal 안에서 commit 순서대로 증가하며 재사용하지 않는다.
pub type BatchId = u64;

/// 활성 writer 세대. 더 큰 세대가 등록되면 이전 세대의 쓰기는 거절된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WriterEpoch(pub u64);

/// append 전에 확인할 stream 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedRevision {
    /// stream에 확정 이벤트가 아직 없어야 한다.
    NoStream,
    /// 마지막 확정 revision이 이 값이어야 한다.
    Exact(Revision),
    /// 확인하지 않는다. 최신 상태에서 판단한 내부 명령에 사용한다.
    Any,
}

/// 불투명 payload. `type_tag`와 `schema_version`으로 codec이 해석한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaquePayload {
    pub type_tag: String,
    pub schema_version: u32,
    pub bytes: Vec<u8>,
}

/// 호출자 범위와 재시도 키. 같은 쌍은 journal 안에서 한 명령만 가리킨다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandKey {
    pub caller_scope: String,
    pub idempotency_key: String,
}

/// 명령의 진행 상태. 종료 상태(Completed·Failed·Cancelled)는 다시 바꿀 수 없다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Accepted,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl CommandStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "accepted" => Self::Accepted,
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// 새로 확정할 명령 기록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommand {
    pub command_id: String,
    /// 재시도 키가 없는 내부 명령은 `None`이다.
    pub key: Option<CommandKey>,
    /// 원본 요청의 식별값. 같은 키로 다른 요청이 오면 충돌로 판정한다.
    pub request_digest: Vec<u8>,
    /// 최초 해소한 대상·입력. 재요청은 이 값을 사용하고 대상을 다시 해소하지 않는다.
    pub resolved: Vec<u8>,
    pub status: CommandStatus,
    /// 응답 재구성 자료. 진행 중이면 비어 있을 수 있다.
    pub response: Option<Vec<u8>>,
}

/// 이미 있는 명령의 진행 갱신.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandUpdate {
    pub command_id: String,
    pub status: CommandStatus,
    pub response: Option<Vec<u8>>,
}

/// 저장된 명령 기록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    pub command_id: String,
    pub key: Option<CommandKey>,
    pub request_digest: Vec<u8>,
    pub resolved: Vec<u8>,
    pub status: CommandStatus,
    pub response: Option<Vec<u8>>,
    /// 이 명령이 만든 batch. commit 순서대로다.
    pub batch_ids: Vec<BatchId>,
}

/// 재시도 키 조회 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLookup {
    /// 처음 보는 키다. 대상을 해소하고 새로 실행한다.
    Miss,
    /// 같은 요청이 이미 있다. 저장된 대상·결과를 사용한다.
    Hit(CommandRecord),
    /// 같은 키가 다른 요청에 쓰였다.
    DigestMismatch(CommandRecord),
}

/// 확정할 이벤트 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    pub event_id: String,
    pub payload: OpaquePayload,
    /// 기록 시각. 순서 판정에는 쓰지 않는다.
    pub recorded_at_ms: u64,
    pub causation_id: Option<String>,
    pub actor: String,
    pub origin: String,
    /// 이 이벤트가 참조하는 불변 payload. 같은 transaction에서 참조로 고정한다.
    pub payload_refs: Vec<PayloadRef>,
}

/// 한 stream에 붙일 이벤트 묶음.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamAppend {
    pub stream_id: StreamId,
    pub expected: ExpectedRevision,
    pub events: Vec<NewEvent>,
}

/// 저장된 이벤트.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub stream_id: StreamId,
    pub stream_revision: Revision,
    pub event_id: String,
    pub batch_id: BatchId,
    pub batch_index: u32,
    pub payload: OpaquePayload,
    pub recorded_at_ms: u64,
    pub command_id: Option<String>,
    pub causation_id: Option<String>,
    pub actor: String,
    pub origin: String,
}

/// batch 경계의 revision vector. batch가 바꾼 stream과 그 뒤 revision이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCut {
    pub batch_id: BatchId,
    pub revisions: BTreeMap<StreamId, Revision>,
}

/// 저장된 batch 전체. 공개 단위이므로 이벤트를 모두 함께 읽는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBatch {
    pub cut: BatchCut,
    pub command_id: Option<String>,
    pub events: Vec<StoredEvent>,
}

/// 한 시점의 전체 journal 경계. 마지막 batch와 모든 stream의 head다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalCut {
    pub last_batch: Option<BatchId>,
    pub heads: BTreeMap<StreamId, Revision>,
}

/// 불변 payload의 참조. id는 content generation이며 재사용하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PayloadRef(pub u64);
