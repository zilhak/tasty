//! PTY 출력 도착 등 비동기 이벤트가 발생했을 때 메인 루프를 깨우는 메커니즘.
//!
//! `WinitWakerFactory` (boot/waker.rs) 가 winit `EventLoopProxy` 로 구현한다.
//! `NoopWakerFactory` 는 plugin manager 등이 wake 없이 동작할 때 쓰는 no-op.
//!
//! 이 추상은 *GUI 이벤트 루프 패턴* 에 종속된 것 — 진짜 headless 영역에선 PTY
//! 리더가 *깨움* 이 아니라 다른 방식으로 동작. 그래서 이 trait 은 본 바이너리에
//! 살고 외부 crate 가 의존하지 않는다.

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
/// 현재 호출처는 plugin manager 의 `#[cfg(test)]` 코드 뿐 — release 빌드에서는
/// dead 가 자연스러움.
#[allow(dead_code)]
pub struct NoopWakerFactory;

impl WakerFactory for NoopWakerFactory {
    fn make_targeted_waker(&self, _surface_id: u32) -> Waker {
        Arc::new(|| {})
    }
    fn make_default_waker(&self) -> Waker {
        Arc::new(|| {})
    }
}
