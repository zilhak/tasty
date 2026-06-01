//! Test mock adapter — `src/ports/` 의 7 external trait 의 deterministic mock.
//!
//! Core 의 단위 test 시 외부 자원 없이 도메인 로직 검증. 모든 in-memory.

#![allow(dead_code)]

pub mod fake_clock;
pub mod mem_fs;
pub mod mock_clipboard;
pub mod mock_ipc_server;
pub mod mock_process;
pub mod mock_pty;
pub mod mock_waker;
pub mod tmp_home;
