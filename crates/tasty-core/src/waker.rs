//! PTY 출력 도착 등 비동기 이벤트가 발생했을 때 메인 루프를 깨우는 메커니즘.
//!
//! 본체(`tasty` 바이너리)는 winit `EventLoopProxy`로 구현하고, 헤드리스/플러그인
//! 호스트 컨텍스트에서는 mpsc 채널 등 다른 메커니즘을 쓸 수 있다. `tasty-core`는
//! 추상 trait만 정의하여 winit 의존을 본체로 격리한다.

use std::sync::Arc;

use tasty_terminal::Waker;

/// surface별 / 일반 waker 생성 인터페이스.
///
/// 본체는 `WinitWakerFactory`로 구현하여 `EngineState`에 주입한다. 헤드리스
/// 테스트나 surface가 PTY 이벤트를 무시해도 되는 컨텍스트는 `NoopWakerFactory`를
/// 쓸 수 있다.
pub trait WakerFactory: Send + Sync + 'static {
    /// 특정 surface의 PTY 데이터 도착 통지용 waker.
    fn make_targeted_waker(&self, surface_id: u32) -> Waker;

    /// surface 식별 없이 "뭔가 도착했음" 일반 waker.
    /// 예: `targeted_pty_polling` 끄고 일괄 처리하는 경우.
    fn make_default_waker(&self) -> Waker;
}

/// 공용 핸들 — `EngineState`에 보관되며 `Arc::clone`으로 PTY 리더 스레드에 분배된다.
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
