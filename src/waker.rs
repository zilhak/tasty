//! PTY 출력 도착 등 비동기 이벤트가 발생했을 때 메인 루프를 깨우는 메커니즘.
//!
//! Trait/Noop 정의는 `tasty_terminal::waker_factory` 로 이동. 본 모듈은 호환용
//! thin re-export 만 유지. `WinitWakerFactory` (boot/waker.rs) 가 winit
//! `EventLoopProxy` 로 production impl 을 제공한다.

pub use tasty_terminal::waker_factory::SharedWakerFactory;
#[cfg(feature = "gui")]
pub use tasty_terminal::waker_factory::WakerFactory;
