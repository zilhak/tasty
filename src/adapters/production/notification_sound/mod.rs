//! GUI는 OS별 소리 재생 구현을, 헤드리스는 NoopPlayer를 사용한다.

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
