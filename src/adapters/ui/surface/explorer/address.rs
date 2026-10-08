//! 주소창 입력을 이동 대상으로 바꾼다. 로컬 입력은 현재 폴더 기준의 절대 경로로 확정해 확인하고,
//! 원격 입력은 로컬 파일시스템을 보지 않고 원격 조회에 맡긴다. 규칙은 docs/surfaces/explorer/index.md의
//! "주소 입력" 절에 있다.

use std::path::{Component, Path, PathBuf};

/// 주소 입력을 이동하지 않은 이유. 사용자에게 toast로 알린다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressRejection {
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    /// 링크 자체는 있지만 대상이 없다.
    BrokenLink(PathBuf),
    /// 권한 거부 등 존재 여부를 확인하지 못했다.
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    /// `~`를 펼칠 홈 폴더를 찾지 못했다.
    NoHome,
    /// 원격 경로의 `~`는 원격 호스트의 홈이므로 로컬 홈으로 펼치지 않는다.
    RemoteHome,
}

impl AddressRejection {
    /// 사용자에게 보일 안내 문구.
    pub fn message(&self) -> String {
        use crate::i18n::{t, t_fmt, t_fmt2};
        let shown = |p: &Path| p.display().to_string();
        match self {
            Self::NotFound(p) => t_fmt("explorer.address.not_found", &shown(p)),
            Self::NotADirectory(p) => t_fmt("explorer.address.not_a_directory", &shown(p)),
            Self::BrokenLink(p) => t_fmt("explorer.state.broken_link", &shown(p)),
            Self::Unreadable { path, reason } => {
                t_fmt2("explorer.address.unreadable", &shown(path), reason)
            }
            Self::NoHome => t("explorer.address.no_home").to_string(),
            Self::RemoteHome => t("explorer.address.remote_home").to_string(),
        }
    }
}

/// 입력을 이동할 폴더로 바꾼다. 빈 입력은 None이다. `remote`면 원격 규칙을 쓴다.
pub fn resolve(
    input: &str,
    current: &Path,
    remote: bool,
) -> Option<Result<PathBuf, AddressRejection>> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if remote {
        return Some(remote_target(input, current));
    }
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    Some(local_path(input, current, home.as_deref()).and_then(|p| check_local_dir(&p).map(|()| p)))
}

/// 로컬 입력을 절대 경로로 확정한다. 파일시스템은 보지 않는다.
/// `~`·`~/…`는 홈, 상대 경로는 `current` 기준이다. 드라이브만 있는 Windows 입력(`C:`, `C:dir`)은
/// 프로세스의 드라이브별 현재 폴더가 아니라 그 드라이브의 루트 기준으로 본다.
/// `.`과 `..`는 글자 그대로 정리한다. 링크를 풀지 않으므로 링크 폴더 안의 `..`는 링크의 부모다.
fn local_path(
    input: &str,
    current: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, AddressRejection> {
    let raw = if let Some(rest) = home_relative(input) {
        home.ok_or(AddressRejection::NoHome)?.join(rest)
    } else {
        let path = PathBuf::from(input);
        let mut parts = path.components();
        match (parts.next(), parts.next()) {
            (Some(Component::Prefix(prefix)), second) if second != Some(Component::RootDir) => {
                let mut rooted = PathBuf::from(prefix.as_os_str());
                rooted.push(std::path::MAIN_SEPARATOR_STR);
                rooted.extend(second);
                rooted.extend(parts);
                rooted
            }
            _ if path.is_absolute() || path.has_root() => path,
            _ => current.join(path),
        }
    };
    Ok(normalize(&raw))
}

/// `~` 또는 `~/…`(Windows는 `~\…`도)면 홈 아래의 나머지를 돌려준다. `~name`은 펼치지 않는다.
fn home_relative(input: &str) -> Option<&str> {
    let rest = input.strip_prefix('~')?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('/')
        .or_else(|| cfg!(windows).then(|| rest.strip_prefix('\\')).flatten())
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push(part);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// 로컬 경로가 이동할 수 있는 폴더인지 확인한다. 폴더를 가리키는 링크는 링크 경로 그대로 연다.
fn check_local_dir(path: &Path) -> Result<(), AddressRejection> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(AddressRejection::NotADirectory(path.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if path.symlink_metadata().is_ok() {
                Err(AddressRejection::BrokenLink(path.to_path_buf()))
            } else {
                Err(AddressRejection::NotFound(path.to_path_buf()))
            }
        }
        Err(e) => Err(AddressRejection::Unreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        }),
    }
}

/// 원격 입력은 원격 호스트의 경로라 로컬에서 존재를 확인하지 않는다. 결과는 원격 목록 조회가 알려 준다.
/// 절대 경로(`/…`, `\…`, `X:\…`, `X:/…`)는 그대로, 나머지는 현재 원격 폴더 기준으로 잇는다.
fn remote_target(input: &str, current: &Path) -> Result<PathBuf, AddressRejection> {
    if input.starts_with('~') {
        return Err(AddressRejection::RemoteHome);
    }
    let bytes = input.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if input.starts_with('/') || input.starts_with('\\') || windows_absolute {
        Ok(PathBuf::from(input))
    } else {
        let dir = current.to_string_lossy();
        Ok(crate::adapters::ui::popup::file_picker::join_dir(true, &dir, input).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(input: &str, current: &str) -> Result<PathBuf, AddressRejection> {
        local_path(input, Path::new(current), Some(Path::new("/home/u")))
    }

    #[cfg(unix)]
    #[test]
    fn local_input_is_absolute_from_the_current_folder_not_the_process() {
        assert_eq!(local("/etc", "/srv/a"), Ok("/etc".into()));
        assert_eq!(local("sub/dir", "/srv/a"), Ok("/srv/a/sub/dir".into()));
        assert_eq!(local("./x/../y", "/srv/a"), Ok("/srv/a/y".into()));
        assert_eq!(local("..", "/srv/a"), Ok("/srv".into()));
        assert_eq!(local("../../../..", "/srv/a"), Ok("/".into()));
        assert_eq!(local("한글 폴더", "/srv/a"), Ok("/srv/a/한글 폴더".into()));
    }

    #[cfg(unix)]
    #[test]
    fn tilde_expands_to_the_local_home_only_on_its_own_or_before_a_separator() {
        assert_eq!(local("~", "/srv"), Ok("/home/u".into()));
        assert_eq!(local("~/docs", "/srv"), Ok("/home/u/docs".into()));
        assert_eq!(local("~other", "/srv"), Ok("/srv/~other".into()));
        assert_eq!(
            local_path("~", Path::new("/srv"), None),
            Err(AddressRejection::NoHome)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_and_unc_inputs_are_rooted() {
        assert_eq!(local(r"C:", r"D:\work"), Ok(r"C:\".into()));
        assert_eq!(local(r"C:dir", r"D:\work"), Ok(r"C:\dir".into()));
        assert_eq!(local(r"C:\dir\..\x", r"D:\work"), Ok(r"C:\x".into()));
        assert_eq!(
            local(r"\\server\share\dir", r"D:\work"),
            Ok(r"\\server\share\dir".into())
        );
        assert_eq!(local(r"sub", r"D:\work"), Ok(r"D:\work\sub".into()));
    }

    #[test]
    fn empty_input_is_no_action() {
        assert_eq!(resolve("   ", Path::new("/"), false), None);
        assert_eq!(resolve("", Path::new("/"), true), None);
    }

    #[test]
    fn local_check_reports_why_a_path_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        let cur = dir.path();
        assert_eq!(
            resolve(" file.txt ", cur, false),
            Some(Err(AddressRejection::NotADirectory(file)))
        );
        assert_eq!(
            resolve("missing", cur, false),
            Some(Err(AddressRejection::NotFound(cur.join("missing"))))
        );
        std::fs::create_dir(cur.join("sub")).unwrap();
        assert_eq!(resolve("sub", cur, false), Some(Ok(cur.join("sub"))));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_link_opens_at_its_own_path_and_a_broken_link_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path();
        std::fs::create_dir(cur.join("real")).unwrap();
        std::os::unix::fs::symlink(cur.join("real"), cur.join("link")).unwrap();
        std::os::unix::fs::symlink(cur.join("gone"), cur.join("broken")).unwrap();
        assert_eq!(resolve("link", cur, false), Some(Ok(cur.join("link"))));
        assert_eq!(
            resolve("broken", cur, false),
            Some(Err(AddressRejection::BrokenLink(cur.join("broken"))))
        );
    }

    #[test]
    fn remote_input_never_looks_at_the_local_filesystem() {
        let missing_here = "/definitely/not/on/this/machine";
        assert_eq!(
            resolve(missing_here, Path::new("/srv"), true),
            Some(Ok(missing_here.into()))
        );
        assert_eq!(
            resolve(r"C:\Users\x", Path::new("/srv"), true),
            Some(Ok(r"C:\Users\x".into()))
        );
        assert_eq!(
            resolve("sub", Path::new("/remote/home"), true),
            Some(Ok("/remote/home/sub".into()))
        );
        assert_eq!(
            resolve("~/x", Path::new("/srv"), true),
            Some(Err(AddressRejection::RemoteHome))
        );
    }

    #[test]
    fn remote_relative_input_follows_the_separator_of_the_remote_folder() {
        assert_eq!(
            resolve("proj", Path::new(r"C:\Users\u"), true),
            Some(Ok(r"C:\Users\u\proj".into()))
        );
        assert_eq!(
            resolve("proj", Path::new(r"C:\"), true),
            Some(Ok(r"C:\proj".into()))
        );
        assert_eq!(
            resolve("proj", Path::new("/"), true),
            Some(Ok("/proj".into()))
        );
    }
}
