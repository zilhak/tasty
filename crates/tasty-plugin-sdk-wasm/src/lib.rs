//! Tasty Plugin SDK — WASM POC runtime (host side).
//!
//! Phase J.C POC. **Main workspace 에서 exclude.** 자체 Cargo.lock 사용.
//!
//! 본 crate 는 host 가 wasi-preview2 component 형식의 plugin 을 로드/호출하는
//! runtime 을 제공한다. wasmtime 45 + component-model + WASI Preview 2 의존.
//!
//! ## 사용 모델 (POC)
//!
//! ```ignore
//! use tasty_plugin_sdk_wasm::{WasmPluginRuntime, HostBridge};
//! let bridge: Box<dyn HostBridge> = Box::new(MyBridge { ... });
//! let mut rt = WasmPluginRuntime::load("clipboard-history.component.wasm", bridge)?;
//! rt.init("com.tasty.clipboard-history", "ko-KR")?;
//! let popup_json = rt.open_popup(r#"{"instance_id": 1, ...}"#)?;
//! ```
//!
//! ## 격리 + 다음 단계
//!
//! 현재 main workspace 에서 exclude 되어 있으므로 본 runtime 은 main 바이너리에
//! 직접 link 되지 않는다. POC 완료 후 정식 채택 시점에 (a) workspace 포함 +
//! tasty-host-plugin 의 wasm.rs 모듈에서 직접 사용 / (b) 별 dylib bridge 중 하나로
//! 통합. 평가 = `docs/evaluations/wasm-poc.md §5`.

pub mod bridge;
pub mod runtime;
pub mod store_state;

pub use bridge::HostBridge;
pub use runtime::WasmPluginRuntime;
pub use store_state::HostState;
