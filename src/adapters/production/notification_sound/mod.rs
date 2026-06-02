//! `NotificationSoundPlayer` production adapter.
//!
//! Platform 별 impl 을 `PlatformPlayer` alias 로 노출. macOS impl 은
//! `objc2-app-kit` (optional, `[features].gui` 게이트) 에 의존하므로
//! `feature = "gui"` 조건이 추가로 붙는다. headless macOS 빌드에서는
//! NoopPlayer alias 로 fallback.

#![allow(dead_code)]

#[cfg(all(target_os = "macos", feature = "gui"))]
pub mod macos;
#[cfg(windows)]
pub mod windows;

#[cfg(all(target_os = "macos", not(feature = "gui")))]
pub use crate::ports::notification_sound::NoopPlayer as PlatformPlayer;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub use macos::MacBeepPlayer as PlatformPlayer;
#[cfg(windows)]
pub use windows::WinBeepPlayer as PlatformPlayer;

// 그 외 OS (Linux/BSD 등) — 후속 substep 에서 Linux impl 추가 전까지
// NoopPlayer fallback.
#[cfg(not(any(target_os = "macos", windows)))]
pub use crate::ports::notification_sound::NoopPlayer as PlatformPlayer;
