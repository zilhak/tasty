//! Host API — tasty 엔진 자체가 사용하지 않고, **외부 (plugin / agent / 사용자)
//! 가 사용하도록 host 가 제공하는 인터페이스 / 인프라**.
//!
//! 각 sub-module 의 본질:
//! - [`hooks`]: 사용자 / plugin 이 등록하는 hook (lua / global)
//! - [`webview`]: native overlay 인프라 — plugin 이 html surface 등 띄울 때 사용
//!
//! inbound adapter 들(IPC/UI/CLI/Plugin, input 포함)은 모두 `src/adapters/` 로
//! 이동 완료. 남은 hooks/webview 는 plugin 시스템의 부속 인프라라 host_api 에 유지.

pub mod hooks;
#[cfg(feature = "gui")]
pub mod webview;
