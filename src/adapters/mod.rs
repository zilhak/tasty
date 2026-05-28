//! Hexagonal architecture 의 *Adapter (port 의 구현)*.
//!
//! - `ipc/` — Inbound adapter (JSON RPC, plugin/CLI 통신)
//! - `production/` — Outbound adapter 의 production 구현 (외부 crate 매핑)
//! - `test/` — Outbound adapter 의 test mock

pub mod ipc;
pub mod production;
#[cfg(test)]
pub mod test;
