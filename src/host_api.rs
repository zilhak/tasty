//! Host API — tasty 엔진 자체가 사용하지 않고, **외부 (plugin / agent / 사용자)
//! 가 사용하도록 host 가 제공하는 인터페이스 / 인프라**.
//!
//! 각 sub-module 의 본질:
//! - [`hooks`]: 사용자 / plugin 이 등록하는 hook (lua / global)
//! - [`webview`]: native overlay 인프라 — plugin 이 html surface 등 띄울 때 사용
//!
//! D.3.B.1 (IPC), D.3.B.2 (UI), D.3.B.3 (CLI), D.3.B.4 (Plugin) 로 inbound
//! adapter 들은 `src/adapters/` 로 모두 이동. 남은 hooks/webview 는 plugin
//! 시스템의 부속 인프라라 host_api 에 유지 (B.5 에서 input 도 ui/input 으로 이동 예정).

pub mod hooks;
pub mod webview;
