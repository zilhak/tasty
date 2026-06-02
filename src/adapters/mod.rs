//! Hexagonal architecture 의 *Adapter (port 의 구현)*.
//!
//! - `ipc/` — Inbound adapter (JSON RPC, plugin/CLI 통신)
//! - `ui/` — Inbound adapter (egui draw, winit window, keyboard/mouse)
//! - `cli/` — Inbound adapter (사용자 shell 진입점 + subcommand)
//! - `plugin/` — Inbound adapter (plugin process 의 manifest/manager/channel)
//! - `production/` — Outbound adapter 의 production 구현 (외부 crate 매핑)
//! - `test/` — Outbound adapter 의 test mock

pub mod cli;
pub mod ipc;
pub mod plugin;
pub mod production;
#[cfg(test)]
pub mod test;
#[cfg(feature = "gui")]
pub mod ui;
