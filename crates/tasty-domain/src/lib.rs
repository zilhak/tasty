#![forbid(unsafe_code)]

//! 구조 저널의 도메인: 저널 전용 구조 모델, 도메인 이벤트, codec, pure evolve.
//!
//! **시험 전용이며 제품에 연결하지 않는다.** 본 바이너리의 `CoreState`가 구조 상태의 유일한
//! 원본이고, 이 크레이트의 [`JournalModel`]은 그와 동시에 원본이 되지 않는다. 필드 대응과
//! 활성화는 제품 배선 단계에서 정한다.
//!
//! - [`JournalModel`]은 workspace·category·pane·tab·surface 트리와 이름·소속·분할 비율·kind·
//!   kind 자료 참조·metadata만 담는다. 선택·포커스·접힘 같은 View 상태는 담지 않는다.
//! - [`evolve`]는 저장된 batch 하나를 통째로 적용한다. 중간에 실패하면 모델은 바뀌지 않는다.
//! - codec은 이벤트마다 type tag와 schema version을 붙이고, 모르는 tag·version은 오류로 중단한다.
//!   분할 비율은 f32 비트를 그대로 저장한다.
//!
//! 저장 계약은 `tasty-event-store`가 맡는다. 이 크레이트는 그 공개 API만 사용한다.

mod codec;
mod event;
mod evolve;
mod ids;
mod model;
mod replay;

#[cfg(test)]
mod tests;

pub use codec::{
    CodecError, EVENT_SCHEMA_VERSION, MODEL_VERSION, decode_event, decode_snapshot, encode_event,
    encode_snapshot,
};
pub use event::{DomainEvent, MetadataTarget, SplitSpec, SurfaceSpec};
pub use evolve::{EvolveError, STRUCTURE_STREAM, evolve};
pub use ids::{CategoryId, IdSupplier, MemoryIdSupplier, PaneId, SurfaceId, TabId, WorkspaceId};
pub use model::{
    Applied, Category, DataRef, JournalModel, Pane, Placement, Ratio, SplitDirection, SplitTree,
    Surface, Tab, Workspace,
};
pub use replay::{ReplayError, full_replay, load, save_snapshot};
