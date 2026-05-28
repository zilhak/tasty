//! Hexagonal architecture 의 *Adapter (port 의 구현)*.
//!
//! - `production/` — 실제 production 에서 사용 (외부 crate 매핑).
//! - `test/` — 단위 test 용 deterministic mock.

pub mod production;
#[cfg(test)]
pub mod test;
