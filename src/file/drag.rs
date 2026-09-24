//! 외부 앱으로 파일을 드래그한다. macOS만 구현됐으며 Windows·Linux는 취소를 반환한다.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::start_file_drag;
#[cfg(target_os = "macos")]
pub use macos::start_file_drag;
#[cfg(windows)]
pub use windows::start_file_drag;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragResult {
    /// macOS의 drop 보고 또는 결과 콜백이 없을 때의 기본값.
    /// 이유: 다른 플랫폼은 취소만 반환하지만 공통 API에 이 값을 유지한다.
    #[allow(dead_code)]
    Accepted,
    /// 취소 보고 또는 아직 구현되지 않은 플랫폼의 반환값.
    Cancelled,
}
