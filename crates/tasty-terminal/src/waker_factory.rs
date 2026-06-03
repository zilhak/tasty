//! Surface/일반 waker 생성 인터페이스.
//!
//! 본 바이너리의 `WinitWakerFactory` (egui/winit 기반) 가 production impl 이며,
//! 헤드리스 컨텍스트는 `NoopWakerFactory` 를 쓴다.
//!
//! 이 모듈은 plugin manager 가 본 바이너리(`crate::waker`) 결합 없이 동일 trait
//! 을 사용할 수 있도록 tasty-terminal 안에 위치한다.

use std::sync::Arc;

use crate::events::Waker;

/// surface 별 / 일반 waker 생성 인터페이스.
///
/// production 경로는 본 바이너리의 winit-기반 impl 로 `CoreState` 에 주입된다.
/// 헤드리스 / 테스트는 [`NoopWakerFactory`] 사용.
pub trait WakerFactory: Send + Sync + 'static {
    /// 특정 surface 의 PTY 데이터 도착 통지용 waker.
    fn make_targeted_waker(&self, surface_id: u32) -> Waker;

    /// surface 식별 없이 "뭔가 도착했음" 일반 waker.
    fn make_default_waker(&self) -> Waker;
}

/// 공용 핸들 — `CoreState` 에 보관되며 `Arc::clone` 으로 PTY 리더 스레드에 분배된다.
pub type SharedWakerFactory = Arc<dyn WakerFactory>;

/// 깨움이 필요 없는 컨텍스트(헤드리스 테스트 등)용 no-op 구현.
pub struct NoopWakerFactory;

impl WakerFactory for NoopWakerFactory {
    fn make_targeted_waker(&self, _surface_id: u32) -> Waker {
        Arc::new(|| {})
    }
    fn make_default_waker(&self) -> Waker {
        Arc::new(|| {})
    }
}
