#![forbid(unsafe_code)]

//! 구조 저널의 도메인: 저널 전용 구조 모델, 도메인 이벤트와 그 codec, pure evolve, decide 계약.
//!
//! **시험 전용이며 제품에 연결하지 않는다.** 본 바이너리의 `CoreState`가 구조 상태의 유일한
//! 원본이고, 이 크레이트의 [`JournalModel`]은 그와 동시에 원본이 되지 않는다. 필드 대응과
//! 활성화는 제품 배선 단계에서 정한다.
//!
//! - journal 하나에 엔진마다 구조 stream이 하나 있다. 이름은 [`STRUCTURE_STREAM_PREFIX`]로 시작한다.
//! - [`JournalModel`]은 엔진 하나의 workspace·category·pane·tab·surface 트리와 이름·소속·분할 비율·
//!   kind·kind 자료 참조를 담는다. workspace 부제·설명·attach 매핑과 tab 명시 이름은 typed 필드이며,
//!   metadata는 사용자 정의 키만 담는다. 선택·포커스·접힘 같은 View 상태는 담지 않는다.
//!   ID와 분할 방향은 `tasty-model`의 타입을 쓴다.
//! - [`evolve`]는 한 stream의 [`DomainBatch`]를, [`evolve_streams`]는 batch 하나를 모든 엔진 모델에
//!   통째로 적용한다. 중간에 실패하면 모델은 바뀌지 않는다. snapshot은 [`StructureModels`] 전체다.
//! - codec은 이벤트 본문에 type tag와 schema version을 붙이고, 모르는 tag·version은 오류로
//!   중단한다. 분할 비율은 f32 비트를 그대로 저장한다.
//! - [`Decider`]는 상태·명령만 보고 결정하는 순수 계약이다. 시각과 command identity는 [`DecisionContext`]로
//!   받는다.
//!
//! 이 크레이트는 이벤트 저장소에 의존하지 않는다. 저장 봉투·저장 형식 버전·migration, 저장
//! batch와 [`DomainBatch`] 사이의 변환, 명령 실행기는 root runtime이 맡는다.

mod codec;
mod command;
mod creation;
mod decider;
mod event;
mod evolve;
mod ids;
mod model;
mod operation;
mod streams;

#[cfg(test)]
mod tests;

pub use codec::{
    CodecError, EVENT_SCHEMA_VERSION, EncodedEvent, MODEL_VERSION, decode_event, decode_snapshot,
    encode_event, encode_snapshot,
};
pub use command::{
    Rejection, StructuralCommand, StructuralDecision, StructuralResult, decide_structure,
};
pub use creation::{
    CleanupPlan, CreationDestination, CreationPlan, PreparationResult, StructuralEffect,
};
pub use decider::{Decider, Decision, DecisionContext};
pub use event::{DomainBatch, DomainEvent, MetadataTarget, RecordedEvent, SplitSpec, SurfaceSpec};
pub use evolve::{EvolveError, evolve};
pub use ids::{BatchId, IdKind, Revision};
pub use model::{
    Applied, Category, DataRef, JournalModel, Pane, Placement, Ratio, SplitTree, Surface, Tab,
    Workspace,
};
pub use operation::{
    Activation, ActivationPhase, EntityId, Operation, OperationId, OperationOutcome,
};
pub use streams::{
    STRUCTURE_STREAM_PREFIX, StreamBatch, StructureModels, evolve_streams, is_structure_stream,
};
