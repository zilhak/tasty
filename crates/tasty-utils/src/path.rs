//! Tasty 의 사용자 데이터 디렉토리 헬퍼.
//!
//! `tasty_home()` 이 기반 경로 (`~/.tasty/`) 를 반환한다. 도메인별 경로
//! (`themes_dir`, `default_db_path`, `config_path` 등) 는 각 도메인 crate 가
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
pub fn tasty_home() -> Option<PathBuf> {
    // 테스트 override는 이 스레드에만 적용하며 환경변수보다 우선한다.
    #[cfg(any(test, feature = "test-support"))]
    if let Some(dir) = home_override::current() {
        return Some(dir);
    }
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

/// 테스트용 홈 경로 스택. 현재 스레드에서는 환경변수보다 우선하며 전역 env를 바꾸지 않는다.
/// 자식 스레드에는 상속되지 않아 그 스레드는 실제 홈이나 TASTY_HOME을 읽을 수 있다.
/// 격리 검증은 docs/dev-guide/unit-test-isolation.md를 따른다.
#[cfg(any(test, feature = "test-support"))]
mod home_override {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static STACK: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn current() -> Option<PathBuf> {
        STACK.with(|s| s.borrow().last().cloned())
    }

    pub(super) fn push(dir: PathBuf) {
        STACK.with(|s| s.borrow_mut().push(dir));
    }

    pub(super) fn pop() {
        STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// 이 스레드의 홈 경로를 추가한다. RAII 가드에서 pop_home_override와 역순으로 짝지어야 한다.
/// 환경변수는 바꾸지 않으며 자식 스레드에 상속하지 않는다.
#[cfg(any(test, feature = "test-support"))]
pub fn push_home_override(dir: PathBuf) {
    home_override::push(dir);
}

/// [`push_home_override`] 로 세운 이 스레드의 마지막 override 를 되돌린다.
#[cfg(any(test, feature = "test-support"))]
pub fn pop_home_override() {
    home_override::pop();
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

/// 해석에 실패한 파일을 백업 이름으로 옮긴다. 읽기 자체가 실패한 파일에는 사용하지 않는다.
/// 성공하면 원래 경로가 비고 이후 저장이 새 파일을 만들 수 있다. 실패하면 덮어쓰면 안 된다.
/// 이미 존재하는 .bak, .bak.2 등을 건너뛰며 상한까지 자리를 찾는다. 이름 선택과 rename은
/// 원자적이지 않으므로 동시 백업에서 기존 파일이 덮어써지지 않는다고 보장하지는 않는다.
/// Ok(None)은 rename 시 원본을 찾지 못했다는 뜻이며, 다른 호출이 먼저 옮겼을 수 있다.
pub fn preserve_corrupt_file(path: &Path) -> std::io::Result<Option<PathBuf>> {
    let Some(candidate) = next_backup_slot(path) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{MAX_BACKUPS} backups already exist for {}", path.display()),
        ));
    };
    match std::fs::rename(path, &candidate) {
        Ok(()) => Ok(Some(candidate)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// 한 파일이 쌓아둘 수 있는 백업 개수(`.bak` + `.bak.2` … `.bak.9`).
const MAX_BACKUPS: u32 = 9;

/// [`preserve_corrupt_file`] 이 지금 쓸 백업 이름. 예산이 소진됐으면 `None`.
fn next_backup_slot(path: &Path) -> Option<PathBuf> {
    let mut candidate = with_appended_extension(path, "bak");
    let mut n = 2;
    while candidate.exists() {
        if n > MAX_BACKUPS {
            return None;
        }
        candidate = with_appended_extension(path, &format!("bak.{n}"));
        n += 1;
    }
    Some(candidate)
}

/// 현재 백업 이름이 모두 사용 중인지 확인한다. 안내 문구 선택용으로만 사용한다.
/// 실제 보존 전 다른 프로세스가 파일을 바꿀 수 있으므로 저장 여부는 보존 결과로 판단한다.
pub fn backup_budget_is_exhausted(path: &Path) -> bool {
    next_backup_slot(path).is_none()
}

/// 기존 확장자를 바꾸지 않고 백업 접미사를 붙인다: config.toml + bak → config.toml.bak.
fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// 홈 경로를 ~/로 줄인 표시용 문자열. 파일 접근에 사용하지 않는다.
pub fn tilde_abbreviate(p: &Path) -> String {
    match os_home_dir().and_then(|home| p.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// 외부 도구 호환을 위해 Windows verbatim 접두사를 제거한다.
/// \\?\UNC\server는 \\server로, \\?\C:\는 C:\로 바꾼다.
/// 일반 경로와 비 Windows 입력은 그대로 반환한다.
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

/// 파일시스템을 조회하지 않고 .과 ..를 정리한다. 루트 위로 올라가지는 않는다.
/// 심볼릭 링크가 있으면 OS가 해석한 경로와 달라질 수 있으므로 표시·중복 제거에 사용한다.
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

    // HOME을 바꾸지 않고 실제 홈을 입력으로 사용해 표시 규칙만 검사한다.
    #[test]
    fn tilde_abbreviate_replaces_home_prefix() {
        let Some(home) = os_home_dir() else {
            return; // 홈을 못 찾는 환경(컨테이너 등)에서는 검증 대상 자체가 없다.
        };
        let expected = format!("~/{}", Path::new("workspace").join("tasty").display());
        assert_eq!(
            tilde_abbreviate(&home.join("workspace").join("tasty")),
            expected
        );
    }

    #[test]
    fn tilde_abbreviate_maps_home_itself_to_bare_tilde() {
        let Some(home) = os_home_dir() else { return };
        // strip_prefix 결과가 빈 경로라 `~/` 로 끝난다 — 표시용이라 이 형태로 굳힌다.
        assert_eq!(tilde_abbreviate(&home), "~/");
    }

    #[test]
    fn tilde_abbreviate_leaves_paths_outside_home_untouched() {
        let Some(home) = os_home_dir() else { return };
        // 홈의 형제 경로 — 홈 접두사가 아니므로 그대로 나와야 한다. 홈이
        // 루트 직하가 아닌 실제 환경에서 부모가 존재한다.
        let outside = home.parent().map_or_else(
            || PathBuf::from("/definitely-not-home"),
            |parent| parent.join("__tasty_not_home__"),
        );
        assert_eq!(tilde_abbreviate(&outside), outside.display().to_string());
    }

    #[test]
    fn tilde_abbreviate_leaves_relative_paths_untouched() {
        // 상대경로는 어떤 절대 홈으로도 strip 되지 않는다.
        let rel = Path::new("relative").join("file.txt");
        assert_eq!(tilde_abbreviate(&rel), rel.display().to_string());
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

    /// 기존 백업을 건너뛰고 다음 번호를 사용하는지 확인한다.
    #[test]
    fn backups_do_not_clobber_each_other() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        std::fs::write(&path, "first").unwrap();
        let first = preserve_corrupt_file(&path).unwrap().unwrap();
        assert_eq!(first, tmp.path().join("config.toml.bak"));

        std::fs::write(&path, "second").unwrap();
        let second = preserve_corrupt_file(&path).unwrap().unwrap();
        assert_eq!(second, tmp.path().join("config.toml.bak.2"));

        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "second");
        assert!(!path.exists(), "원본은 자리를 떠야 다음 저장이 안전하다");
    }

    /// 원본이 이미 없으면 이동할 파일이 없다는 결과를 반환한다.
    #[test]
    fn already_moved_file_is_not_a_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("config.toml");
        assert_eq!(preserve_corrupt_file(&missing).unwrap(), None);
    }
}
