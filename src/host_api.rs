//! Host API — tasty 엔진 자체가 사용하지 않고, **외부 (plugin / agent / 사용자)
//! 가 사용하도록 host 가 제공하는 인터페이스 / 인프라**.
//!
//! 각 sub-module 의 본질:
//! - [`cli`]: 사용자 shell 진입점 + plugin 이 contribute 하는 subcommand 등록 표면
//! - [`hooks`]: 사용자 / plugin 이 등록하는 hook (lua / global)
//! - [`ipc`]: JSON-RPC 인프라 — 외부 (agent / plugin process) 가 호출
//! - [`plugin`]: plugin 시스템 (manifest / manager / process / channel)
//! - [`webview`]: native overlay 인프라 — plugin 이 html surface 등 띄울 때 사용
//!
//! main.rs 에서 `pub(crate) use host_api::*;` 형식으로 재노출되어, 호출처는
//! `crate::ipc::*` 등의 기존 경로 그대로 사용한다.

pub mod cli;
pub mod hooks;
pub mod ipc;
pub mod plugin;
pub mod webview;
