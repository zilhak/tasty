//! Tasty 공용 도메인 데이터 크레이트 (GUI-free).
//!
//! - `model`: 워크스페이스/탭/팬/서피스 등 도메인 모델, 길이 타입
//! - `i18n`: 번역 로더 + `t()` 함수
//! - `agent_id`: 잠정 agent 식별자 (Phase 4 관측/비용)
//!
//! 이동 완료:
//! - `paths::tasty_home()` → `tasty-utils::path`
//! - `waker` (WakerFactory) → 본 바이너리 `src/waker.rs` (GUI 종속이라)
//!
//! **GUI-free.** 이 crate 는 시각 표현(색, sizing, theme) 을 절대 알지 않는다.
//! 그건 `tasty-type-appearance` (schema) + `tasty-themes` (도메인/IO) 의 책임.

pub mod agent_id;
pub mod i18n;
pub mod model;

pub use agent_id::AgentId;

/// `Surface::as_any` / `as_any_mut` 구현을 한 줄로 채우는 매크로.
///
/// ```ignore
/// impl Surface for MyPanel {
///     tasty_core::impl_surface_any!();
///     // ... 다른 메서드들 ...
/// }
/// ```
#[macro_export]
macro_rules! impl_surface_any {
    () => {
        fn as_any(&self) -> &dyn ::std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
            self
        }
    };
}
