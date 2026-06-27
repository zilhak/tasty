//! 플랫폼 특정 모듈 + crash report (전 플랫폼) + embedded icon/asset.

#[cfg(feature = "gui")]
pub mod app_icon;
pub mod crash_report;
#[cfg(all(debug_assertions, feature = "gui"))]
pub mod debug_info;
#[cfg(all(windows, feature = "gui"))]
pub mod jump_list;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub mod macos_delegate;
#[cfg(feature = "gui")]
pub mod native_menu;
#[cfg(all(windows, feature = "gui"))]
pub mod power_windows;
#[cfg(feature = "gui")]
pub mod reveal;
#[cfg(all(
    any(windows, target_os = "macos", target_os = "linux"),
    feature = "gui"
))]
pub mod system_tray;
#[cfg(feature = "gui")]
pub mod window_chrome;
