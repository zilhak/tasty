//! Tasty 의 사용자 데이터 디렉토리 헬퍼.
//!
//! `tasty_home()` 이 기반 경로 (`~/.tasty/`) 를 반환한다. 도메인별 경로
//! (`themes_dir`, `memory_db_path`, `config_path` 등) 는 각 도메인 crate 가
//! 이 함수를 호출해 자체 정의한다 — utils 는 *공통 기반* 만 제공.

use std::path::PathBuf;

use directories::BaseDirs;

/// Tasty 의 사용자 데이터 디렉토리. 모든 플랫폼에서 `~/.tasty/`.
///
/// AI 에이전트가 경로를 외우기 쉽게 단일화 (Linux 규약상 `~/.config/tasty/` 가
/// 자연스럽지만, agent 접근성 우선).
pub fn tasty_home() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty"))
}

/// 자식 프로세스·외부 도구에 넘길 경로에서 Windows verbatim(extended-length,
/// `\\?\`) prefix 를 제거한다.
///
/// `std::fs::canonicalize` 는 Windows 에서 `\\?\C:\...` 형태의 verbatim 경로를
/// 돌려준다. 이 경로가 자식 PTY 의 working_dir 로 전달되면 자식의 `process.cwd()`
/// 가 `\\?\...` 가 되는데, 일부 도구(예: Claude Code/bun 의 `pathToFileURL`)는
/// 이를 file URL 로 변환하지 못해 깨진다. 그래서 외부로 내보내는 경로는 일반
/// 형태로 되돌린다.
///
/// - `\\?\UNC\server\share\..` → `\\server\share\..`
/// - `\\?\C:\..`               → `C:\..`
/// - 이미 일반 경로 / 비-Windows → 입력 그대로 (no-op)
///
/// strip 로직은 `#[cfg(windows)]` 안에만 존재하므로 다른 플랫폼의 동작은 변하지
/// 않는다.
pub fn strip_verbatim_prefix(p: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = p.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    p.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn strips_verbatim_disk_prefix() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\E:\workspace\tasty\.worktree\wt-1"),
            r"E:\workspace\tasty\.worktree\wt-1"
        );
    }

    #[test]
    #[cfg(windows)]
    fn strips_verbatim_unc_prefix() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\dir"),
            r"\\server\share\dir"
        );
    }

    #[test]
    #[cfg(windows)]
    fn leaves_normal_path_untouched() {
        assert_eq!(
            strip_verbatim_prefix(r"E:\already\normal"),
            r"E:\already\normal"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn noop_on_non_windows() {
        // 비-Windows 에서는 어떤 입력이든 그대로 반환한다.
        assert_eq!(strip_verbatim_prefix("/usr/lib/tasty"), "/usr/lib/tasty");
        assert_eq!(strip_verbatim_prefix(r"\\?\X"), r"\\?\X");
    }
}
