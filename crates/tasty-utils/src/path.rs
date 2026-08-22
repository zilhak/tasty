//! Tasty 의 사용자 데이터 디렉토리 헬퍼.
//!
//! `tasty_home()` 이 기반 경로 (`~/.tasty/`) 를 반환한다. 도메인별 경로
//! (`themes_dir`, `memory_db_path`, `config_path` 등) 는 각 도메인 crate 가
//! 이 함수를 호출해 자체 정의한다 — utils 는 *공통 기반* 만 제공.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

/// Tasty 의 사용자 데이터 디렉토리.
///
/// 우선순위:
/// 1. 환경변수 `TASTY_HOME` 이 비어있지 않으면 그 경로를 그대로 루트로 사용
///    (임시 루트 override — 테스트/샌드박스/다중 인스턴스용).
/// 2. fallback: debug 빌드 → `~/.tasty-debug/`, release 빌드 → `~/.tasty/`.
///    루트 자체를 갈라 debug 인스턴스가 release 데이터(state.db / layout.json /
///    memory.db / plugins / 포트파일 등)를 공유·오염하지 않게 격리한다.
///
/// AI 에이전트가 경로를 외우기 쉽게 단일화 (Linux 규약상 `~/.config/tasty/` 가
/// 자연스럽지만, agent 접근성 우선).
pub fn tasty_home() -> Option<PathBuf> {
    // 1) TASTY_HOME 환경변수 우선 (임시 루트 override)
    if let Ok(custom) = std::env::var("TASTY_HOME") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    // 2) fallback: debug → ~/.tasty-debug, release → ~/.tasty
    let dirname = if cfg!(debug_assertions) {
        ".tasty-debug"
    } else {
        ".tasty"
    };
    BaseDirs::new().map(|dirs| dirs.home_dir().join(dirname))
}

/// OS 사용자 홈 디렉토리 — **tasty 데이터 루트가 아니다.**
///
/// [`tasty_home`] 은 `~/.tasty{-debug}`(또는 `TASTY_HOME` override)를 돌려주므로
/// 홈 자체가 필요한 경로(`~/.ssh/config` 등 tasty 밖의 규약 경로)에는 쓸 수 없다.
/// 이 함수는 override 를 보지 않는다 — 다른 프로그램(ssh 등)이 읽는 경로를 계산하는
/// 용도라, tasty 의 테스트용 루트 이동이 그 프로그램의 홈까지 옮기지는 않기 때문이다.
pub fn os_home_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
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

/// 경로의 `.` / `..` 세그먼트를 **순수 lexical** 로(파일시스템 접근 없이) 붕괴시킨다.
///
/// `std::path::absolute` 와의 차이: `absolute` 는 Unix 에서 symlink 안전성 때문에
/// `..` 를 보존한다(`/a/b/../c` 를 `/a/c` 로 줄이면 `b` 가 symlink 일 때 실제 대상과
/// 달라지므로). 이 함수는 **모든 플랫폼에서 동일하게** `..` 를 붕괴시킨다 —
/// markdown 링크의 표시/dedup 용도에는 lexical 붕괴가 사용자 의도에 맞고 dedup 키가
/// 안정적이다.
///
/// 트레이드오프: 붕괴 대상 세그먼트가 symlink 이면 lexical 결과가 OS 의 실제 해석과
/// 달라질 수 있다(`b` 가 symlink 면 `/a/b/../c` ≠ `/a/c`). markdown 링크 용도에서는
/// 이 동작이 의도된 것이며 허용된다. 루트/prefix 위로는 올라가지 않는다.
pub fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                // 앞선 일반 세그먼트를 pop 해서 `..` 를 붕괴.
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // 루트/prefix 위로는 올라갈 수 없다 — `..` 무시.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                // 상대경로 선두의 `..` 는 붕괴 대상이 없으므로 보존.
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 경로 문자열의 백슬래시 구분자를 `/` 로 통일한다.
///
/// `file_format` 의 `PathGlob` 패턴처럼 저장·비교를 OS 와 무관하게 항상 `/` 로
/// 해야 하는 문자열에 쓴다 — 입력이 어느 OS 에서 작성됐는지 알 수 없으므로(예:
/// 동기화된 설정 파일) **실행 OS 와 무관하게 항상** 변환한다. 실제 파일시스템
/// 호출 등 OS 경계에서만 [`from_slash`] 로 되돌린다.
pub fn to_slash(p: &str) -> String {
    p.replace('\\', "/")
}

/// [`to_slash`] 의 역변환 — `/` 를 **현재 OS** 네이티브 구분자로 되돌린다.
/// `Path`/`PathBuf` 와 주고받는 지점(OS 경계)에서만 쓴다. non-Windows 는 이미
/// `/` 가 네이티브 구분자라 no-op.
pub fn from_slash(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\")
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn normalizes_parent_and_cur_segments() {
        assert_eq!(
            lexically_normalize(Path::new("/docs/md/../sibling.md")),
            PathBuf::from("/docs/sibling.md")
        );
        assert_eq!(
            lexically_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        // 루트 위로는 못 올라간다.
        assert_eq!(lexically_normalize(Path::new("/../x")), PathBuf::from("/x"));
        // 상대경로 선두의 `..` 는 보존.
        assert_eq!(
            lexically_normalize(Path::new("../a/b")),
            PathBuf::from("../a/b")
        );
    }

    #[test]
    #[cfg(windows)]
    fn normalizes_parent_segments_windows() {
        assert_eq!(
            lexically_normalize(Path::new(r"C:\docs\md\..\sibling.md")),
            PathBuf::from(r"C:\docs\sibling.md")
        );
    }

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

    #[test]
    fn to_slash_replaces_backslash_regardless_of_os() {
        assert_eq!(to_slash(r"src\file\format.rs"), "src/file/format.rs");
        assert_eq!(to_slash("already/slash"), "already/slash");
        assert_eq!(to_slash(r"mixed/a\b"), "mixed/a/b");
    }

    #[test]
    #[cfg(windows)]
    fn from_slash_restores_backslash_on_windows() {
        assert_eq!(from_slash("src/file/format.rs"), r"src\file\format.rs");
    }

    #[test]
    #[cfg(not(windows))]
    fn from_slash_is_noop_on_non_windows() {
        assert_eq!(from_slash("src/file/format.rs"), "src/file/format.rs");
    }
}
