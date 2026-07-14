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

use std::ffi::OsString;
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

/// `base` PATH(보통 부모 프로세스의 PATH) 맨 앞에 현재 실행 중인 바이너리의
/// 디렉토리를 붙인 새 PATH 값을 만든다.
///
/// 자식 프로세스가 `tasty` CLI 자기 자신을 이름으로 호출할 수 있게 하기 위한 것.
/// 패키징된 macOS `.app`(LaunchServices 경유 실행)은 PATH 를
/// `/usr/bin:/bin:/usr/sbin:/sbin` 로 제한해 `tasty` 바이너리 디렉토리
/// (`.../Tasty.app/Contents/MacOS`)가 빠지므로, 이 보강 없이는 자식(터미널 셸,
/// hook 셸 커맨드 등)이 `tasty` 를 못 찾아 `command not found` 로 실패한다.
///
/// 크로스플랫폼 구분자는 [`std::env::split_paths`] / [`std::env::join_paths`] 가
/// 처리한다(unix `:`, windows `;`). `current_exe()` 실패·parent 없음·join 실패 시
/// `None` — 호출부는 PATH 를 건드리지 않고 상속 그대로 둔다(패닉 금지). self-binary
/// 디렉토리 하나만 추가하며, 로그인쉘/사용자 커스텀 PATH 복제는 하지 않는다.
pub fn path_prepending_self_dir(base: Option<OsString>) -> Option<OsString> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut entries = vec![dir.to_path_buf()];
    if let Some(base) = base.as_deref() {
        entries.extend(std::env::split_paths(base));
    }
    std::env::join_paths(entries).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// self-binary 디렉토리가 최소 PATH 맨 앞에 붙는지 검증한다 — 패키징된
    /// macOS `.app` 이 받는 `/usr/bin:/bin:...` 같은 최소 PATH 를 흉내낸다.
    #[test]
    fn prepends_self_binary_dir_to_minimal_path() {
        let exe = std::env::current_exe().expect("current_exe");
        let self_dir = exe.parent().expect("parent");
        // join_paths 로 만들어 구분자(`:`/`;`)를 크로스플랫폼으로 맞춘다.
        let base = std::env::join_paths([
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/bin"),
        ])
        .expect("join base");

        let result = path_prepending_self_dir(Some(base)).expect("path built");
        let entries: Vec<_> = std::env::split_paths(&result).collect();

        assert_eq!(
            entries.first().map(|p| p.as_path()),
            Some(self_dir),
            "self binary dir must be the first PATH entry"
        );
        // 기존 항목도 보존되어야 한다(덮어쓰기가 아니라 prepend).
        assert!(
            entries
                .iter()
                .any(|p| p == std::path::Path::new("/usr/bin")),
            "existing PATH entries must be preserved"
        );
    }

    /// base 가 없어도(부모에 PATH env 자체가 없는 드문 케이스) self-dir 만으로
    /// 유효한 PATH 를 만든다.
    #[test]
    fn builds_path_from_self_dir_when_base_absent() {
        let result = path_prepending_self_dir(None).expect("path built");
        let entries: Vec<_> = std::env::split_paths(&result).collect();
        assert_eq!(entries.len(), 1, "only the self binary dir");
    }
}
