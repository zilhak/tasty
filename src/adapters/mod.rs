//! Hexagonal architecture 의 *Adapter (port 의 구현)*.
//!
//! - `production/` — 실제 production 에서 사용 (외부 crate 매핑).
//! - `test/` — 단위 test 용 deterministic mock.
//!
//! Phase D 진행 중. D.3.A.2 에서 production, D.3.A.3 에서 test mock 추가.

pub mod production;
