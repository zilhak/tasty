//! 주소창 입력을 이동 대상으로 바꾼다. 로컬 입력은 현재 폴더 기준의 절대 경로로 확정해 확인하고,
//! 원격 입력은 로컬 파일시스템을 보지 않고 원격 조회에 맡긴다. 규칙은 docs/surfaces/explorer/index.md의
//! "주소 입력" 절에 있다.

pub(super) mod field;
mod probe;
mod remote;

pub(super) use probe::AddressProbe;

use std::path::{Component, Path, PathBuf};

/// 주소를 해석할 파일시스템. 원격 홈은 연결마다 한 번 조회하며 아직 모르면 None이다.
#[derive(Clone, Copy, Debug)]
pub enum Host<'a> {
    Local,
    Remote { home: Option<&'a Path> },
}

/// 주소 입력이 가리키는 이동 대상.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressTarget {
    /// 폴더로 이동한다.
    Folder(PathBuf),
    /// 파일이 든 폴더로 이동해 그 파일을 고른다.
    File { dir: PathBuf, file: PathBuf },
    /// 원격 경로. 폴더인지 파일인지는 부모 폴더의 원격 목록으로 확인한 뒤 이동한다.
    Remote(PathBuf),
}

/// 주소 입력을 이동하지 않은 이유. 사용자에게 toast로 알린다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressRejection {
    NotFound(PathBuf),
    /// 권한 거부 등 존재 여부를 확인하지 못했다.
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    /// `~`를 펼칠 홈 폴더를 찾지 못했다.
    NoHome,
    /// 원격 홈을 아직 모른다(조회 대기·실패). 원격 `~`는 로컬 홈으로 펼치지 않는다.
    RemoteHome,
}

impl AddressRejection {
    /// 사용자에게 보일 안내 문구.
    pub fn message(&self) -> String {
        use crate::i18n::{t, t_fmt, t_fmt2};
        let shown = |p: &Path| p.display().to_string();
        match self {
            Self::NotFound(p) => t_fmt("explorer.address.not_found", &shown(p)),
            Self::Unreadable { path, reason } => {
                t_fmt2("explorer.address.unreadable", &shown(path), reason)
            }
            Self::NoHome => t("explorer.address.no_home").to_string(),
            Self::RemoteHome => t("explorer.address.remote_home").to_string(),
        }
    }
}

/// 입력을 이동 대상으로 바꾼다. 빈 입력은 None이다. 원격이면 원격 규칙을 쓴다.
pub fn resolve(
    input: &str,
    current: &Path,
    host: Host<'_>,
) -> Option<Result<AddressTarget, AddressRejection>> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Host::Remote { home } = host {
        return Some(remote_target(input, current, home).map(AddressTarget::Remote));
    }
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    Some(local_path(input, current, home.as_deref()).and_then(check_local))
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

/// 로컬 경로의 이동 대상을 정한다. 폴더는 그 폴더로, 파일은 그 파일이 든 폴더로 간다. 링크는 풀지 않고
/// 링크 경로 그대로 쓴다. 폴더를 가리키는 링크는 그 링크를 열고, 파일을 가리키는 링크와 대상이 없는 링크는
/// 링크가 든 폴더에서 링크를 고른다. 대상이 없는 링크도 목록에 항목으로 보이기 때문이다.
fn check_local(path: PathBuf) -> Result<AddressTarget, AddressRejection> {
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => Ok(AddressTarget::Folder(path)),
        Ok(_) => Ok(file_in_folder(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if path.symlink_metadata().is_ok() {
                Ok(file_in_folder(path))
            } else {
                Err(AddressRejection::NotFound(path))
            }
        }
        Err(e) => Err(AddressRejection::Unreadable {
            path,
            reason: e.to_string(),
        }),
    }
}

/// 폴더가 아닌 항목을 그 항목이 든 폴더로 가서 고르는 대상. 절대 경로라 부모가 없는 경우는 루트뿐이고
/// 루트는 폴더이므로 여기 오지 않는다.
fn file_in_folder(file: PathBuf) -> AddressTarget {
    match file.parent() {
        Some(dir) => AddressTarget::File {
            dir: dir.to_path_buf(),
            file,
        },
        None => AddressTarget::Folder(file),
    }
}

/// 원격 입력은 원격 호스트의 경로라 로컬에서 존재를 확인하지 않는다. 결과는 원격 목록 조회가 알려 준다.
/// `~`·`~/…`는 원격 홈, 절대 경로(`/…`, `\…`, `X:\…`, `X:/…`)는 그대로, 나머지는 현재 원격 폴더 기준이다.
/// 그 뒤 `.`·`..`를 글자 그대로 정리한다.
fn remote_target(
    input: &str,
    current: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, AddressRejection> {
    let bytes = input.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let raw = if let Some(rest) = remote::home_relative(input) {
        let home = home.ok_or(AddressRejection::RemoteHome)?;
        remote::under_home(&home.to_string_lossy(), rest)
    } else if input.starts_with('/') || input.starts_with('\\') || windows_absolute {
        input.to_string()
    } else {
        let dir = current.to_string_lossy();
        crate::adapters::ui::popup::file_picker::join_dir(true, &dir, input)
    };
    Ok(remote::normalize(&raw).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_HOME: Host<'static> = Host::Remote { home: None };

    fn remote_with_home(input: &str, current: &str, home: &str) -> PathBuf {
        let target = resolve(
            input,
            Path::new(current),
            Host::Remote {
                home: Some(Path::new(home)),
            },
        )
        .unwrap()
        .unwrap();
        match target {
            AddressTarget::Remote(path) => path,
            other => panic!("원격 입력이 원격 대상이 아니다: {other:?}"),
        }
    }

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
        assert_eq!(resolve("   ", Path::new("/"), Host::Local), None);
        assert_eq!(resolve("", Path::new("/"), NO_HOME), None);
    }

    #[test]
    fn a_file_path_opens_its_folder_and_picks_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/한글 파일.md"), b"x").unwrap();
        let cur = dir.path();
        assert_eq!(
            resolve(" file.txt ", cur, Host::Local),
            Some(Ok(AddressTarget::File {
                dir: cur.to_path_buf(),
                file
            }))
        );
        assert_eq!(
            resolve("sub/./한글 파일.md", cur, Host::Local),
            Some(Ok(AddressTarget::File {
                dir: cur.join("sub"),
                file: cur.join("sub/한글 파일.md")
            }))
        );
    }

    #[test]
    fn local_check_reports_why_a_path_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path();
        assert_eq!(
            resolve("missing", cur, Host::Local),
            Some(Err(AddressRejection::NotFound(cur.join("missing"))))
        );
        std::fs::create_dir(cur.join("sub")).unwrap();
        assert_eq!(
            resolve("sub", cur, Host::Local),
            Some(Ok(AddressTarget::Folder(cur.join("sub"))))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_resolved_and_a_broken_link_is_picked_in_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path();
        std::fs::create_dir(cur.join("real")).unwrap();
        std::fs::write(cur.join("real/notes.md"), b"n").unwrap();
        std::os::unix::fs::symlink(cur.join("real"), cur.join("link")).unwrap();
        std::os::unix::fs::symlink(cur.join("real/notes.md"), cur.join("note-link")).unwrap();
        std::os::unix::fs::symlink(cur.join("gone"), cur.join("broken")).unwrap();
        assert_eq!(
            resolve("link", cur, Host::Local),
            Some(Ok(AddressTarget::Folder(cur.join("link"))))
        );
        // 파일 링크는 대상이 든 폴더가 아니라 링크가 든 폴더에서 링크를 고른다.
        assert_eq!(
            resolve("note-link", cur, Host::Local),
            Some(Ok(AddressTarget::File {
                dir: cur.to_path_buf(),
                file: cur.join("note-link")
            }))
        );
        assert_eq!(
            resolve("link/notes.md", cur, Host::Local),
            Some(Ok(AddressTarget::File {
                dir: cur.join("link"),
                file: cur.join("link/notes.md")
            }))
        );
        // 대상이 없는 링크도 목록에 항목으로 보이므로 링크가 든 폴더에서 고른다.
        assert_eq!(
            resolve("broken", cur, Host::Local),
            Some(Ok(AddressTarget::File {
                dir: cur.to_path_buf(),
                file: cur.join("broken")
            }))
        );
    }

    #[test]
    fn remote_input_never_looks_at_the_local_filesystem() {
        let missing_here = "/definitely/not/on/this/machine";
        assert_eq!(
            resolve(missing_here, Path::new("/srv"), NO_HOME),
            Some(Ok(AddressTarget::Remote(missing_here.into())))
        );
        assert_eq!(
            resolve(r"C:\Users\x", Path::new("/srv"), NO_HOME),
            Some(Ok(AddressTarget::Remote(r"C:\Users\x".into())))
        );
        assert_eq!(
            resolve("sub", Path::new("/remote/home"), NO_HOME),
            Some(Ok(AddressTarget::Remote("/remote/home/sub".into())))
        );
        assert_eq!(
            resolve("~/x", Path::new("/srv"), NO_HOME),
            Some(Err(AddressRejection::RemoteHome))
        );
    }

    #[test]
    fn remote_relative_input_follows_the_separator_of_the_remote_folder() {
        assert_eq!(
            resolve("proj", Path::new(r"C:\Users\u"), NO_HOME),
            Some(Ok(AddressTarget::Remote(r"C:\Users\u\proj".into())))
        );
        assert_eq!(
            resolve("proj", Path::new(r"C:\"), NO_HOME),
            Some(Ok(AddressTarget::Remote(r"C:\proj".into())))
        );
        assert_eq!(
            resolve("proj", Path::new("/"), NO_HOME),
            Some(Ok(AddressTarget::Remote("/proj".into())))
        );
    }

    #[test]
    fn remote_tilde_is_the_remote_home_once_it_is_known() {
        assert_eq!(
            remote_with_home("~", "/srv", "/home/r"),
            PathBuf::from("/home/r")
        );
        assert_eq!(
            remote_with_home("~/docs/../src", "/srv", "/home/r"),
            PathBuf::from("/home/r/src")
        );
        assert_eq!(
            remote_with_home(r"~\docs", r"D:\work", r"C:\Users\r"),
            PathBuf::from(r"C:\Users\r\docs")
        );
        assert_eq!(
            remote_with_home("~other", "/srv", "/home/r"),
            PathBuf::from("/srv/~other")
        );
    }

    #[test]
    fn remote_dots_are_resolved_with_the_remote_separator_not_the_local_one() {
        assert_eq!(
            remote_with_home("..", "/srv/a", "/h"),
            PathBuf::from("/srv")
        );
        assert_eq!(
            remote_with_home("./x/../y", "/srv/a", "/h"),
            PathBuf::from("/srv/a/y")
        );
        assert_eq!(
            remote_with_home("../../../..", "/srv/a", "/h"),
            PathBuf::from("/")
        );
        assert_eq!(
            remote_with_home(r"..\x", r"C:\Users\u", "/h"),
            PathBuf::from(r"C:\Users\x")
        );
        assert_eq!(
            remote_with_home("/etc/./ssh/..", "/srv", "/h"),
            PathBuf::from("/etc")
        );
    }
}
