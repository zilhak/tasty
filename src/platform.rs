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
// 순수한 목록 결정 로직이 macOS 밖에서도 유닛테스트되도록 모듈 자체는 gui 빌드 전체에서
// 컴파일한다 — 실제 파일 접근부만 모듈 안에서 macOS 로 좁힌다.
#[cfg(feature = "gui")]
pub mod macos_permissions;
#[cfg(feature = "gui")]
pub mod native_menu;
#[cfg(all(windows, feature = "gui"))]
pub mod power_windows;
#[cfg(feature = "gui")]
pub mod reveal;
#[cfg(feature = "gui")]
pub mod screen_capture;
#[cfg(all(
    any(windows, target_os = "macos", target_os = "linux"),
    feature = "gui"
))]
pub mod system_tray;
#[cfg(feature = "gui")]
pub mod window_chrome;
