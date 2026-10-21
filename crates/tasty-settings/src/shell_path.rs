//! 설정한 셸 경로의 판정. 런타임 대체, 부팅 판정, 설정 창 경고, 첫 실행 셸 설정 화면이 같은 판정을 쓴다.
//!
//! macOS·Linux는 셸 이름을 보지 않는다. fish처럼 bash·zsh가 아닌 셸도 실행할 수 있으면 쓴다.
//! 경로가 디렉터리이거나 실행 권한이 없으면 원인을 따로 알린다. 이 두 경우는 다른 셸로 대신하지
//! 않는다 — 사용자가 고른 경로가 왜 실패하는지 보여 주고 고치게 한다.
//! Windows는 파일이 있고 이름에 bash 또는 zsh가 들어 있어야 쓴다(실행 비트가 없다).

use std::path::Path;

/// 셸 경로 판정 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellPathStatus {
    /// 값이 비었다. 실행할 때 탐지한 셸을 쓴다.
    Empty,
    /// 경로에 아무것도 없다. 실행할 때 탐지한 셸로 대신한다.
    Missing,
    /// Windows 전용. 파일은 있지만 이름에 bash·zsh가 없다. 실행할 때 탐지한 셸로 대신한다.
    NotShell,
    /// macOS·Linux 전용. 경로가 디렉터리다. 대신하지 않고 실행에 실패한다.
    Directory,
    /// macOS·Linux 전용. 파일에 실행 권한이 없다. 대신하지 않고 실행에 실패한다.
    NotExecutable,
    /// 쓸 수 있는 셸이다.
    Usable,
}

impl ShellPathStatus {
    /// `path`를 판정한다.
    pub fn of(path: &str) -> Self {
        if path.is_empty() {
            return Self::Empty;
        }
        let path = Path::new(path);
        if !path.exists() {
            return Self::Missing;
        }
        #[cfg(windows)]
        {
            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if filename.contains("bash") || filename.contains("zsh") {
                Self::Usable
            } else {
                Self::NotShell
            }
        }
        #[cfg(not(windows))]
        {
            if path.is_dir() {
                return Self::Directory;
            }
            if executable(path) {
                Self::Usable
            } else {
                Self::NotExecutable
            }
        }
    }

    /// 실행할 때 탐지한 셸로 대신하는 판정인지. 실행 불가(디렉터리·권한 없음)는 대신하지 않는다.
    pub fn falls_back(self) -> bool {
        matches!(self, Self::Empty | Self::Missing | Self::NotShell)
    }
}

/// 이 프로세스가 `path`를 실행할 수 있는지. portable-pty가 실행 전에 쓰는 검사와 같은 `access(X_OK)`다.
#[cfg(not(windows))]
fn executable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c_path는 이 호출 동안 살아 있는 NUL 종료 문자열이다. access는 경로를 읽기만 한다.
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::ShellPathStatus;

    #[test]
    fn an_empty_value_and_a_missing_path_fall_back() {
        assert_eq!(ShellPathStatus::of(""), ShellPathStatus::Empty);
        assert_eq!(
            ShellPathStatus::of("/nonexistent/tasty-shell-path/fish"),
            ShellPathStatus::Missing
        );
        assert!(ShellPathStatus::Empty.falls_back());
        assert!(ShellPathStatus::Missing.falls_back());
        assert!(ShellPathStatus::NotShell.falls_back());
    }

    #[test]
    fn unusable_but_present_paths_do_not_fall_back() {
        assert!(!ShellPathStatus::Directory.falls_back());
        assert!(!ShellPathStatus::NotExecutable.falls_back());
        assert!(!ShellPathStatus::Usable.falls_back());
    }

    #[cfg(not(windows))]
    mod unix {
        use super::ShellPathStatus;
        use std::os::unix::fs::PermissionsExt;

        fn file_with_mode(dir: &std::path::Path, name: &str, mode: u32) -> String {
            let path = dir.join(name);
            std::fs::write(&path, "#!/bin/sh\n").expect("write fixture");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .expect("chmod fixture");
            path.to_string_lossy().into_owned()
        }

        /// 이름에 bash·zsh가 없어도 실행할 수 있으면 쓴다.
        #[test]
        fn an_executable_named_fish_is_usable() {
            let dir = tempfile::tempdir().expect("tempdir");
            let fish = file_with_mode(dir.path(), "fish", 0o755);
            assert_eq!(ShellPathStatus::of(&fish), ShellPathStatus::Usable);
        }

        #[test]
        fn a_directory_is_reported_as_a_directory() {
            let dir = tempfile::tempdir().expect("tempdir");
            assert_eq!(
                ShellPathStatus::of(&dir.path().to_string_lossy()),
                ShellPathStatus::Directory
            );
        }

        #[test]
        fn a_file_without_execute_permission_is_not_executable() {
            let dir = tempfile::tempdir().expect("tempdir");
            let plain = file_with_mode(dir.path(), "zsh", 0o644);
            assert_eq!(ShellPathStatus::of(&plain), ShellPathStatus::NotExecutable);
        }
    }

    /// Windows 현행 이름 조건. 이 시험은 Windows 빌드에서만 돈다.
    #[cfg(windows)]
    mod windows {
        use super::ShellPathStatus;

        fn file(dir: &std::path::Path, name: &str) -> String {
            let path = dir.join(name);
            std::fs::write(&path, "").expect("write fixture");
            path.to_string_lossy().into_owned()
        }

        #[test]
        fn only_names_with_bash_or_zsh_are_usable() {
            let dir = tempfile::tempdir().expect("tempdir");
            assert_eq!(
                ShellPathStatus::of(&file(dir.path(), "bash.exe")),
                ShellPathStatus::Usable
            );
            assert_eq!(
                ShellPathStatus::of(&file(dir.path(), "zsh.exe")),
                ShellPathStatus::Usable
            );
            assert_eq!(
                ShellPathStatus::of(&file(dir.path(), "cmd.exe")),
                ShellPathStatus::NotShell
            );
            assert_eq!(
                ShellPathStatus::of(&file(dir.path(), "fish.exe")),
                ShellPathStatus::NotShell
            );
        }
    }
}
