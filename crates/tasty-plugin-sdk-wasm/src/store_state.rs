//! wasmtime Store 에 보관하는 host 측 상태.
//!
//! WASI Preview 2 의 `WasiCtx` + 본 POC 의 `HostBridge` 를 함께 보관.
//! linker 의 closure 가 `store.data_mut()` 로 접근.

use std::sync::Arc;

use wasmtime_wasi::ResourceTable;
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiView};

use crate::bridge::HostBridge;

pub struct HostState {
    pub wasi: WasiCtx,
    pub table: ResourceTable,
    pub bridge: Arc<dyn HostBridge + Send + Sync>,
}

impl HostState {
    pub fn new(bridge: Arc<dyn HostBridge + Send + Sync>) -> Self {
        // Sandbox 검증의 핵심: WasiCtx 를 *최소 권한* 으로 빌드.
        //   - preopen 0 (filesystem 차단)
        //   - inherit_stdio 만 (stdio 도 host 측 통제 가능)
        //   - sockets / clocks / random 은 기본값 (random 은 plugin 내부 PRNG 시드용 허용)
        // wasi-preview2 의 capability injection 모델 — *주입 안 한 것은 사용 불가*.
        let wasi = WasiCtx::builder().inherit_stdio().build();
        Self {
            wasi,
            table: ResourceTable::new(),
            bridge,
        }
    }
}

impl IoView for HostState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}
