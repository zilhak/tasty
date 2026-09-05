//! Event Bus 1.0 wire 타입.
//!
//! 호스트와 plugin은 [`EventEnvelope`]로 사건을 주고받는다. 페이로드 스키마는
//! [`payloads`] 모듈의 Rust 타입으로 정의되어 있고, envelope 안에는
//! `serde_json::Value`로 직렬화되어 실린다.
//!
//! 카탈로그(이벤트 키 → 페이로드 스키마 → scope → 등급 매핑)는
//! `docs/reference/event-catalog.md`가 단일 출처(SoT)다.

mod envelope;

pub mod payloads;

pub use envelope::{EventEnvelope, EventMeta, EventOrigin, EventScope, LifecycleReason, MAX_HOP};
