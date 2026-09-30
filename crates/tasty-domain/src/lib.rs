#![forbid(unsafe_code)]

//! 구조 저널의 도메인: 저널 전용 구조 모델, 도메인 이벤트와 그 codec, pure evolve, decide 계약.
//!
//! **시험 전용이며 제품에 연결하지 않는다.** 본 바이너리의 `CoreState`가 구조 상태의 유일한
//! 원본이고, 이 크레이트의 [`JournalModel`]은 그와 동시에 원본이 되지 않는다. 필드 대응과
//! 활성화는 제품 배선 단계에서 정한다.
//!
//! - [`JournalModel`]은 workspace·category·pane·tab·surface 트리와 이름·소속·분할 비율·kind·
//!   kind 자료 참조·metadata만 담는다. 선택·포커스·접힘 같은 View 상태는 담지 않는다.
//!   ID와 분할 방향은 `tasty-model`의 타입을 쓴다.
//! - [`evolve`]는 해석을 마친 [`DomainBatch`] 하나를 통째로 적용한다. 중간에 실패하면 모델은
//!   바뀌지 않는다.
//! - codec은 이벤트 본문에 type tag와 schema version을 붙이고, 모르는 tag·version은 오류로
//!   중단한다. 분할 비율은 f32 비트를 그대로 저장한다.
//! - [`Decider`]는 상태·명령만 보고 결정하는 순수 계약이다. 새 ID와 시각은 [`DecisionContext`]로
//!   받는다.
//!
//! 이 크레이트는 이벤트 저장소에 의존하지 않는다. 저장 봉투·저장 형식 버전·migration, 저장
//! batch와 [`DomainBatch`] 사이의 변환, 명령 실행기는 root runtime이 맡는다.

mod codec;
mod decider;
mod event;
mod evolve;
mod ids;
mod model;

#[cfg(test)]
mod tests;

pub use codec::{
    CodecError, EVENT_SCHEMA_VERSION, EncodedEvent, MODEL_VERSION, decode_event, decode_snapshot,
    encode_event, encode_snapshot,
};
pub use decider::{Decider, Decision, DecisionContext};
pub use event::{DomainBatch, DomainEvent, MetadataTarget, RecordedEvent, SplitSpec, SurfaceSpec};
pub use evolve::{EvolveError, STRUCTURE_STREAM, evolve};
pub use ids::{BatchId, IdKind, IdSupplier, MemoryIdSupplier, Revision};
pub use model::{
    Applied, Category, DataRef, JournalModel, Pane, Placement, Ratio, SplitTree, Surface, Tab,
    Workspace,
};
