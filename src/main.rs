#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;

/// dhat-heap feature로 할당을 계측하고 종료 시 dhat-heap.json을 기록한다.
/// 사용법: docs/dev-guide/memory-leak-soak.md.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    tasty::boot::run()
}
