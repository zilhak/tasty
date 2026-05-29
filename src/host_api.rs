//! Host API — tasty 엔진 자체가 사용하지 않고, **외부 (plugin / agent / 사용자)
//! 가 사용하도록 host 가 제공하는 인터페이스 / 인프라**.
//!
//! 각 sub-module 의 본질:
//! - [`hooks`]: 사용자 / plugin 이 등록하는 hook (lua / global)
//! - [`plugin`]: plugin 시스템 (manifest / manager / process / channel)
//! - [`webview`]: native overlay 인프라 — plugin 이 html surface 등 띄울 때 사용
//!
//! D.3.B.1 (IPC), D.3.B.2 (UI), D.3.B.3 (CLI) 로 inbound adapter 들은
//! `src/adapters/` 로 모두 이동됨. 남은 cli/ipc/ui 외 도메인 정리는 D.3.B.4~B.5.

pub mod hooks;
pub mod plugin;
pub mod webview;
