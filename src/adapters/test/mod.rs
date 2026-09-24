//! 외부 자원 없이 도메인을 검증하는 시험용 port 구현. 시험 빌드에서만 사용한다.

pub mod fake_clock;
pub mod mem_fs;
pub mod mock_clipboard;
pub mod mock_ipc_server;
pub mod mock_process;
pub mod mock_waker_factory;
pub mod tmp_home;
