//! Interface for surface-specific and general wake callbacks.
//! The host supplies GUI or headless implementations; tests may use NoopWakerFactory.

use std::sync::Arc;

use crate::events::Waker;

/// Creates wake callbacks without coupling plugins to the host event loop.
pub trait WakerFactory: Send + Sync + 'static {
    /// 특정 surface 의 PTY 데이터 도착 통지용 waker.
    fn make_targeted_waker(&self, surface_id: u32) -> Waker;

    /// surface 식별 없이 "뭔가 도착했음" 일반 waker.
    fn make_default_waker(&self) -> Waker;

    /// dedup 게이트 리셋: `Some(sid)` 면 해당 surface 의 게이트, `None` 이면 글로벌
    /// default 게이트를 푼다. event handler 가 PTY 채널 drain *직전* 에 호출해야,
    /// drain 과 경합하는 wake 가 스킵되어 유실되는 것을 막는다.
    fn note_drained(&self, surface_id: Option<u32>);

    /// surface 가 닫힐 때 호출 — 해당 surface 의 dedup 게이트를 내부 맵에서 제거한다.
    /// 호출하지 않으면 게이트가 프로세스 수명 동안 surface 마다 누적된다(누수).
    /// 기본 구현은 no-op (게이트를 보관하지 않는 impl 용).
    fn forget_surface(&self, _surface_id: u32) {}
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
    fn note_drained(&self, _surface_id: Option<u32>) {}
}
