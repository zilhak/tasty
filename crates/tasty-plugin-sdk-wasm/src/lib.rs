//! 호스트에서 WASI Preview 2 컴포넌트를 실행하는 실험용 런타임.
//! 메인 workspace와 바이너리에 포함되지 않는다. 의존 버전은 Cargo.toml에 있다.
//!
//! 사용 예:
//!
//! ```ignore
//! use tasty_plugin_sdk_wasm::{WasmPluginRuntime, HostBridge};
//! let bridge: std::sync::Arc<dyn HostBridge + Send + Sync> = std::sync::Arc::new(MyBridge { ... });
//! let mut rt = WasmPluginRuntime::load("clipboard-history.component.wasm", bridge)?;
//! rt.init("com.tasty.clipboard-history", "ko-KR")?;
//! let popup_json = rt.open_popup(r#"{"instance_id": 1, ...}"#)?;
//! ```
//!
//! 정식 지원 정책: docs/dev-guide/plugin-packaging.md#정책-현행.

pub mod bridge;
pub mod runtime;
pub mod store_state;

pub use bridge::HostBridge;
pub use runtime::WasmPluginRuntime;
pub use store_state::HostState;
