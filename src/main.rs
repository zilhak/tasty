// 헤드리스 빌드는 창이 없으므로 콘솔 서브시스템으로 남겨 셸이 종료를 기다리고 출력을 받게 한다.
#![cfg_attr(
    all(windows, not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

use anyhow::Result;

/// dhat-heap feature로 할당을 계측하고 종료 시 dhat-heap.json을 기록한다.
/// 사용법: docs/dev-guide/memory-leak-soak.md.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<std::process::ExitCode> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    tasty::boot::run()
}
