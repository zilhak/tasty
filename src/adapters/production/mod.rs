//! Production adapter — `src/ports/` 의 7 external trait 의 실제 구현.
//!
//! Phase D 진행 중 — D.3.A.5 (Core 가 trait object 보유) 전까지 호출처 0.
//! 그 때까지 dead_code 경고 억제.

#![allow(dead_code)]

#[cfg(feature = "gui")]
pub mod arboard_clip;
pub mod directories_home;
pub mod portable_pty;
pub mod std_clock;
pub mod std_fs;
pub mod std_process;
pub mod tcp_ipc_server;
#[cfg(feature = "gui")]
pub mod winit_waker;
