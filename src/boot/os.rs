//! OS 별 부팅 보정.
//!
//! - Windows: `AttachConsole` 로 부모 콘솔에 붙어 stdout 보임 (release only).
//! - 전 플랫폼: `crash_report::init` 으로 패닉 → 로그 파일 핸들러 등록 + stderr tracing.
//! - host 경로 한정: `enable_host_file_log` 로 공유 로그 파일 개방 (CLI 는 열지 않는다).
//! - macOS: `macos_delegate::store_proxy` 로 NSApplicationDelegate 에 event loop proxy 보관.

#[cfg(feature = "gui")]
use winit::event_loop::EventLoopProxy;

#[cfg(feature = "gui")]
use crate::AppEvent;

/// release 빌드 Windows 에서만 `AttachConsole(ATTACH_PARENT_PROCESS)` 호출.
/// 다른 환경에서는 no-op.
pub(crate) fn attach_windows_console_if_needed() {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // SAFETY: AttachConsole은 thread-safe Win32 호출. main 진입 첫 단계로,
        // 다른 thread가 아직 spawn되지 않은 시점. 결과 무시는 의도적 (부모가 GUI 셸이면 실패).
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS); // 부모가 GUI 셸이면 Err — 콘솔 부재는 정상, 무시.
        }
    }
}

/// 패닉 → crash log 파일 핸들러 등록 + tracing(stderr) 초기화. 전 플랫폼.
///
/// 공유 로그 **파일**은 여기서 열지 않는다 — [`enable_host_file_log`] 참고.
pub(crate) fn init_crash_report() {
    crate::crash_report::init();
}

/// 공유 로그 파일(`$TASTY_HOME/debug{-dev}.log`)을 열어 파일 tracing 을 켠다.
///
/// **host(GUI / headless) 경로에서만** 부른다. CLI 클라이언트도 같은 바이너리라,
/// 역할 판정 전에 열면 CLI 를 한 번 돌릴 때마다 실행 중인 host 의 로그가 truncate 된다
/// (`docs/adr/0092-file-log-host-process-only.md`).
pub(crate) fn enable_host_file_log() {
    crate::crash_report::enable_host_file_log();
}

/// macOS NSApplicationDelegate 가 dock click 등에서 본 app 으로 이벤트를 보낼 수 있도록
/// event loop proxy 를 보관. macOS 외에서는 no-op. gui 빌드 전용.
#[cfg(feature = "gui")]
#[allow(unused_variables)]
pub(crate) fn install_macos_delegate(proxy: &EventLoopProxy<AppEvent>) {
    #[cfg(target_os = "macos")]
    crate::macos_delegate::store_proxy(proxy.clone());
}
