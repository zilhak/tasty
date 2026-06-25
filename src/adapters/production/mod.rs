//! Production adapter — `src/ports/` 의 7 external trait 의 실제 구현.
//!
//! Core 가 이 구현들을 trait object 로 보유해(D.3.A.5) 실제 호출 경로가 있으므로,
//! 일반 빌드에서 사용된다 — dead_code allow 가 필요 없다.

#[cfg(feature = "gui")]
pub mod arboard_clip;
pub mod directories_home;
#[cfg(not(feature = "gui"))]
pub mod headless_waker;
pub mod notification_sound;
pub mod std_clock;
pub mod std_fs;
pub mod std_process;
pub mod stream_hub;
pub mod tcp_ipc_server;
