//! 자식 프로세스 생성 시 공통 설정.
//!
//! Tasty 본체는 Windows release 빌드에서 GUI 서브시스템
//! (`windows_subsystem = "windows"`) 으로 동작한다. 이때 콘솔 서브시스템
//! 자식 프로세스(플러그인 바이너리, `cmd.exe`/`sh` 훅 등)를 그냥 spawn 하면
//! Windows 가 매번 *새 콘솔 창* 을 할당해 화면에 빈 터미널이 쏟아진다.
//!
//! [`hide_console`] 을 spawn 직전에 호출하면 `CREATE_NO_WINDOW` 플래그가 붙어
//! 콘솔 창이 뜨지 않는다. 비-Windows 에서는 no-op 이라 호출부에서 cfg 분기를
//!둘 필요가 없다.
//!
//! 주의: pane 안에서 실제로 도는 사용자 셸(bash/cmd 등)은 `portable-pty` 의
//! ConPTY 경로로 생성되므로 이 헬퍼와 무관하다. 여기서 다루는 것은 호스트가
//! *백그라운드로* 띄우는 보조 프로세스뿐이다.

use std::process::Command;

/// Windows 콘솔 서브시스템 자식 프로세스가 새 콘솔 창을 띄우지 않도록
/// `CREATE_NO_WINDOW` 플래그를 적용한다. 비-Windows 에서는 아무 동작도 하지 않는다.
///
/// `Command` 를 mutable 로 받아 그대로 돌려주므로 빌더 체인 중간에 끼울 수 있다.
pub fn hide_console(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // winbase.h CREATE_NO_WINDOW. windows-sys 의존 회피용 직접 상수.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
