//! 호스트의 surface kind 레지스트리를 plugin manager 가 의존 없이 받기 위한 trait.
//!
//! 호스트 측 concrete `SurfaceKindRegistry` 가 본 바이너리에 잔존(엔진/모델 의존이
//! 깊다)하고 이 trait 의 impl 을 갖는다. plugin manager 는 `Arc<dyn SurfaceRegistry>`
//! 만 가지고 있어, manager crate (`tasty-host-plugin`) 분리 시 본 바이너리 결합이 끊긴다.
//!
//! 본 trait 는 manager 가 실제로 사용하는 *최소 표면* 만 노출한다.
//! 동적 등록(closure 포함) 은 호스트 측 concrete 경유 — 호스트 glue 가 직접 호출한다.

pub trait SurfaceRegistry: Send + Sync {
    /// 등록된 surface kind 인지 확인. plugin manager 가 활성화 가능 여부 검증에 사용.
    fn contains(&self, kind: &str) -> bool;
}
