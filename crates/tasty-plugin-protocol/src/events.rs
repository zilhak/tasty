//! 호스트와 플러그인이 주고받는 이벤트 형식.
//! EventEnvelope 안에 payloads의 타입을 JSON 값으로 담는다.
//! 이벤트 목록: docs/reference/event-catalog.md.

mod envelope;

pub mod payloads;

pub use envelope::{EventEnvelope, EventMeta, EventOrigin, EventScope, LifecycleReason, MAX_HOP};
