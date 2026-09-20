#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;

/// dhat heap 프로파일링 (opt-in): `cargo build --features dhat-heap` 빌드에서
/// 전 할당을 계측해 종료 시 `dhat-heap.json` 을 남긴다. UMDH 없는 환경에서의
/// 크로스플랫폼 heap attribution 용 — docs/dev-guide/memory-leak-soak.md 참조.
/// release 배포 경로와 무관 (기본 feature 아님).
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    tasty::boot::run()
}
