//! `NotificationSoundPlayer` production adapter.
//!
//! GUI 실행만 OS 별 impl 을 `PlatformPlayer` alias 로 주입한다. headless 는
//! `boot::wiring::build_production_core_headless` 가 `NoopPlayer` 를 직접 넣으므로
//! 플랫폼 구현 모듈을 컴파일하지 않는다 — 그쪽에 소비자가 한 곳도 없다
//! (docs/dev-guide/headless-build-boundaries.md). port 의 trait 계약은 공용이다.

#![allow(unused_imports)]

#[cfg(all(target_os = "linux", feature = "gui"))]
pub mod linux;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub mod macos;
#[cfg(all(windows, feature = "gui"))]
pub mod windows;

#[cfg(all(target_os = "macos", not(feature = "gui")))]
pub use crate::ports::notification_sound::NoopPlayer as PlatformPlayer;
#[cfg(all(target_os = "linux", feature = "gui"))]
pub use linux::LinuxBeepPlayer as PlatformPlayer;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub use macos::MacBeepPlayer as PlatformPlayer;
#[cfg(all(windows, feature = "gui"))]
pub use windows::WinBeepPlayer as PlatformPlayer;

// 그 외 OS (BSD 등) — NoopPlayer fallback.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub use crate::ports::notification_sound::NoopPlayer as PlatformPlayer;
