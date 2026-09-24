//! Core와 Hub가 외부 자원에 접근하는 인터페이스.
//! 제품 어댑터와 시험용 구현은 src/adapters에 두고 생성 시 주입한다.
//! 메모리·프리셋·설정·테마 저장소의 인터페이스는 각 workspace 크레이트에 있다.

pub mod clipboard;
pub mod clock;
pub mod fs;
pub mod home;
pub mod ipc_server;
pub mod notification_sound;
pub mod process;
