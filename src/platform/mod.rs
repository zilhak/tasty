//! 플랫폼 특정 모듈 + crash report (전 플랫폼) + embedded icon/asset.

pub mod app_icon;
pub mod crash_report;
#[cfg(debug_assertions)]
pub mod debug_info;
#[cfg(windows)]
pub mod jump_list;
#[cfg(target_os = "macos")]
pub mod macos_delegate;
#[cfg(windows)]
pub mod system_tray;
