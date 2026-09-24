//! 외부 시스템을 연결하는 src/ports trait의 실제 구현.

#[cfg(feature = "gui")]
pub mod arboard_clip;
pub mod directories_home;
#[cfg(not(feature = "gui"))]
pub mod headless_waker;
pub mod notification_sound;
pub mod std_clock;
pub mod std_fs;
pub mod std_process;
pub mod tcp_ipc_server;
