#![cfg(feature = "wasm-poc")]
//! 기본적으로 꺼져 있는 wasm-poc 실험용 인터페이스. 실제 호스트 로더는 아직 구현하지 않았다.
//! 독립 실행 실험은 workspace 밖의 tasty-plugin-sdk-wasm에서 수행한다.
//! 현재 제품 지원 범위는 docs/dev-guide/plugin-packaging.md#정책-현행 참고.
//!
//!
//! ```bash
//! ./scripts/build-wasm-plugin.sh
//! cargo run --release --manifest-path crates/tasty-plugin-sdk-wasm/Cargo.toml \
//!     --bin poc-host -- target/poc/clipboard-history.component.wasm
//! ```

/// 향후 호스트 연동을 위한 최소 인터페이스. 실행 기능은 제공하지 않는다.
pub trait WasmPluginInstance {
    fn plugin_id(&self) -> &str;
}
