//! Test mock adapter — `src/ports/` 의 7 external trait 의 deterministic mock.
//!
//! Core 의 단위 test 시 외부 자원 없이 도메인 로직 검증. 모든 in-memory.

// SAFETY-net: 이 mock adapter 는 core 단위 test(cfg(test)) 에서만 쓰인다.
// 일반 `cargo build` 에는 일부 mock 메서드가 미사용으로 보이지만 test 빌드에선 사용된다.
#![allow(dead_code)]

pub mod fake_clock;
pub mod mem_fs;
pub mod mock_clipboard;
pub mod mock_ipc_server;
pub mod mock_process;
pub mod tmp_home;
