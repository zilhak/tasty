//! Tasty 공용 데이터/타입 크레이트.
//!
//! - `model`: 워크스페이스/탭/팬/서피스 등 도메인 모델, 길이 타입
//! - `theme`: Catppuccin 팔레트, UI 상수
//! - `i18n`: 번역 로더 + `t()` 함수
//! - `paths`: `~/.tasty/` 등 공용 경로 헬퍼

pub mod color;
pub mod i18n;
pub mod model;
pub mod paths;
pub mod theme;
pub mod waker;

pub use waker::{NoopWakerFactory, SharedWakerFactory, WakerFactory};

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
