#![cfg(feature = "wasm-poc")]
//! Phase J.C WASM POC — host-side loader 자리표시.
//!
//! 현재 POC 단계는 `crates/tasty-plugin-sdk-wasm/` 의 standalone `poc-host`
//! 바이너리로 분리되어 있다 (main workspace 에서 exclude 되어 wasmtime 의존성이
//! 본 crate 로 새지 않도록 격리).
//!
//! 본 모듈은 `wasm-poc` feature 활성 시점에 *production 통합* 단계의 인터페이스
//! 자리표시. 정식 채택 의사결정 (= `docs/adr/0009-plugin-sandbox-deferred.md`)
//! 후에 본 모듈이 sdk-wasm 의 `WasmPluginRuntime` 을 import 해서
//! `PluginProcess` 와 동등한 채널 인터페이스 (`PluginEvent` mpsc, `PluginRequest`
//! mpsc) 를 노출하도록 구현 예정.
//!
//! # 현재 상태
//!
//! - `wasm-poc` feature 는 *기본 비활성*. `cargo build` / `cargo build
//!   --workspace` 모두 본 모듈 미컴파일 → main binary 표면 변화 0.
//! - 본 모듈은 stub 으로만 존재. 실제 wasm 실행은 standalone harness:
//!
//! ```bash
//! ./scripts/build-wasm-plugin.sh
//! cargo run --release --manifest-path crates/tasty-plugin-sdk-wasm/Cargo.toml \
//!     --bin poc-host -- target/poc/clipboard-history.component.wasm
//! ```
//!
//! # 정식 채택 시 구현 윤곽
//!
//! ```ignore
//! pub struct WasmPluginInstance {
//!     rt: tasty_plugin_sdk_wasm::WasmPluginRuntime,
//!     pub req_tx: mpsc::Sender<PluginRequest>,
//!     pub resp_rx: mpsc::Receiver<PluginResponse>,
//!     pub event_rx: mpsc::Receiver<PluginEvent>,
//! }
//!
//! impl WasmPluginInstance {
//!     pub fn load(component_path: &Path, package: &PluginPackage) -> Result<Self> {
//!         let bridge = Arc::new(HostHandleBridge::new(...));
//!         let mut rt = WasmPluginRuntime::load(component_path, bridge)?;
//!         rt.init(&package.id, locale)?;
//!         // → background thread that polls req_rx / dispatches to rt / pushes to resp_tx
//!         ...
//!     }
//! }
//! ```
//!
//! # 정식 채택 결정 조건
//!
//! `wasm-poc.md §0 TL;DR` 의 5 라인 결론에 따라 결정.
//! POC 측정 후 trigger:
//! - 모든 지표 < 1.3x process : 도입 검토 가능 → 본 stub 을 실구현으로 교체.
//! - cold-start > 2x process : 도입 보류 → 본 stub 삭제.

/// 자리표시 — production 통합 단계에서 `PluginProcess` 의 메서드 시그니처와
/// 동일한 표면을 노출하기 위한 trait skeleton.
pub trait WasmPluginInstance {
    fn plugin_id(&self) -> &str;
}
