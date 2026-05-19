//! 플랫폼 특정 모듈 + crash report (전 플랫폼).

pub mod crash_report;
#[cfg(windows)]
pub mod jump_list;
#[cfg(target_os = "macos")]
pub mod macos_delegate;
#[cfg(windows)]
pub mod system_tray;
