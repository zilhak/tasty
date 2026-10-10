//! 원격 주소 입력의 `~` 풀이와 `.`·`..` 정리. 원격 경로는 로컬 OS의 `Path` 규칙으로 다루지 않는다.
//! 구분자는 원격 현재 경로를 따른다(`\` 나 드라이브 접두가 있으면 Windows 형식). 규칙은
//! docs/surfaces/explorer/index.md의 "주소 입력" 절에 있다.

use std::path::PathBuf;

use super::super::view::{ExplorerView, ExplorerViewStore};
use super::Host;
use crate::adapters::ui::popup::file_picker::join_dir;

impl ExplorerView {
    /// 주소창 입력을 해석할 파일시스템. 원격이면 연결 때 조회한 홈을 함께 넘긴다.
    pub(crate) fn address_host(&self, remote: bool) -> Host<'_> {
        if remote {
            Host::Remote {
                home: self.remote_home.as_deref(),
            }
        } else {
            Host::Local
        }
    }
}

impl ExplorerViewStore {
    /// 새 연결의 홈 조회를 기록한다. 이전 연결의 홈과 대기 중인 조회는 버린다.
    pub(crate) fn begin_home_probe(&mut self, local_ws_id: u32, request_id: u64) {
        self.remote_homes.remove(&local_ws_id);
        self.home_probes.retain(|_, ws| *ws != local_ws_id);
        self.home_probes.insert(request_id, local_ws_id);
    }

    /// mirror workspace의 원격 홈. 조회 전이거나 실패했으면 None이다.
    pub(crate) fn remote_home(&self, local_ws_id: u32) -> Option<&std::path::Path> {
        self.remote_homes.get(&local_ws_id).map(PathBuf::as_path)
    }

    /// 홈 조회의 응답이면 홈을 저장하고 true를 반환한다. 실패 응답이면 홈을 모르는 채로 둔다.
    pub(crate) fn finish_home_probe(&mut self, request_id: u64, home: Option<&str>) -> bool {
        let Some(ws) = self.home_probes.remove(&request_id) else {
            return false;
        };
        if let Some(home) = home.filter(|h| !h.is_empty()) {
            self.remote_homes.insert(ws, PathBuf::from(home));
        }
        true
    }
}

/// `~` 또는 `~/…`·`~\…`면 홈 아래의 나머지를 돌려준다. 원격 OS를 모르므로 두 구분자를 모두 받는다.
/// `~name`은 다른 사용자의 홈을 뜻할 수 있어 펼치지 않는다.
pub(super) fn home_relative(input: &str) -> Option<&str> {
    let rest = input.strip_prefix('~')?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('/').or_else(|| rest.strip_prefix('\\'))
}

/// 홈 기준 입력을 홈 경로의 구분자로 잇는다.
pub(super) fn under_home(home: &str, rest: &str) -> String {
    if rest.is_empty() {
        home.to_string()
    } else {
        join_dir(true, home, rest)
    }
}

/// 드라이브 접두(`C:`)나 `\` 가 있으면 Windows 형식으로 본다.
fn is_windows_style(path: &str) -> bool {
    let b = path.as_bytes();
    path.contains('\\') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// 경로를 루트와 나머지로 나눈다. Windows 형식이면 드라이브 루트(`C:\`)와 UNC 공유(`\\host\share\`)를
/// 루트로 본다.
fn split_root(path: &str, windows: bool) -> (String, &str) {
    if !windows {
        return match path.strip_prefix('/') {
            Some(rest) => ("/".to_string(), rest),
            None => (String::new(), path),
        };
    }
    let is_sep = |c: char| c == '\\' || c == '/';
    let b = path.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return (format!("{}:\\", &path[..1]), &path[2..]);
    }
    if let Some(rest) = path
        .strip_prefix("\\\\")
        .or_else(|| path.strip_prefix("//"))
    {
        let mut parts = rest.splitn(3, is_sep);
        if let (Some(host), Some(share)) = (parts.next(), parts.next())
            && !host.is_empty()
            && !share.is_empty()
        {
            return (format!("\\\\{host}\\{share}\\"), parts.next().unwrap_or(""));
        }
    }
    match path.strip_prefix(is_sep) {
        Some(rest) => ("\\".to_string(), rest),
        None => (String::new(), path),
    }
}

/// `.`·`..`를 글자 그대로 정리한다. 원격 링크는 풀지 않으므로 링크 폴더 안의 `..`는 링크의 부모다.
/// 루트 위의 `..`는 루트에 머문다. 빈 구간과 끝의 구분자는 없앤다.
pub(super) fn normalize(path: &str) -> String {
    let windows = is_windows_style(path);
    let (root, rest) = split_root(path, windows);
    let sep = if windows { "\\" } else { "/" };
    let mut parts: Vec<&str> = Vec::new();
    for seg in rest.split(|c: char| c == '/' || (windows && c == '\\')) {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if root.is_empty() {
                    parts.push("..");
                }
            }
            name => parts.push(name),
        }
    }
    let body = parts.join(sep);
    if root.is_empty() && body.is_empty() {
        ".".to_string()
    } else {
        format!("{root}{body}")
    }
}

/// 정리한 원격 경로를 부모 폴더와 마지막 이름으로 나눈다. 루트이거나 루트 없는 한 구간이면 None이다.
/// 빈 경로를 보내면 서버가 홈을 읽으므로 부모가 빈 문자열이 되는 경우는 만들지 않는다.
pub(super) fn parent_and_name(path: &str) -> Option<(String, String)> {
    let windows = is_windows_style(path);
    let (root, rest) = split_root(path, windows);
    let is_sep = |c: char| c == '/' || (windows && c == '\\');
    let rest = rest.trim_matches(is_sep);
    let (head, name) = match rest.rfind(is_sep) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("", rest),
    };
    let parent = format!("{root}{head}");
    (!name.is_empty() && !parent.is_empty()).then(|| (parent, name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_dots_are_resolved_lexically_and_stop_at_the_root() {
        assert_eq!(normalize("/srv/a/./b/../c"), "/srv/a/c");
        assert_eq!(normalize("/srv/a/.."), "/srv");
        assert_eq!(normalize("/srv/../../.."), "/");
        assert_eq!(normalize("/srv//a/"), "/srv/a");
        assert_eq!(normalize("/"), "/");
        assert_eq!(normalize("/srv/a b/한글/.."), "/srv/a b");
    }

    #[test]
    fn windows_dots_keep_the_drive_or_share_root_and_use_backslashes() {
        assert_eq!(normalize(r"C:\Users\u\..\x"), r"C:\Users\x");
        assert_eq!(normalize(r"C:\.."), r"C:\");
        assert_eq!(normalize("C:/Users/u/./docs"), r"C:\Users\u\docs");
        assert_eq!(normalize(r"C:\Users\u\a/b"), r"C:\Users\u\a\b");
        assert_eq!(normalize(r"\\host\share\dir\..\.."), r"\\host\share\");
        assert_eq!(normalize(r"\dir\..\x"), r"\x");
    }

    #[test]
    fn a_path_without_a_root_keeps_leading_parents() {
        assert_eq!(normalize("a/../.."), "..");
        assert_eq!(normalize("."), ".");
    }

    #[test]
    fn a_remote_path_splits_into_its_folder_and_name_by_the_remote_format() {
        let split = parent_and_name;
        assert_eq!(
            split("/srv/a/notes.md"),
            Some(("/srv/a".into(), "notes.md".into()))
        );
        assert_eq!(split("/notes.md"), Some(("/".into(), "notes.md".into())));
        assert_eq!(
            split(r"C:\Users\u\a.txt"),
            Some((r"C:\Users\u".into(), "a.txt".into()))
        );
        assert_eq!(split(r"C:\a.txt"), Some((r"C:\".into(), "a.txt".into())));
        assert_eq!(
            split(r"\\host\share\a.txt"),
            Some((r"\\host\share\".into(), "a.txt".into()))
        );
        assert_eq!(
            split("/srv/a b/한글.md"),
            Some(("/srv/a b".into(), "한글.md".into()))
        );
        assert_eq!(split("/"), None);
        assert_eq!(split(r"C:\"), None);
        assert_eq!(split("notes.md"), None);
    }

    #[test]
    fn only_a_bare_tilde_or_one_before_a_separator_is_the_home() {
        assert_eq!(home_relative("~"), Some(""));
        assert_eq!(home_relative("~/docs"), Some("docs"));
        assert_eq!(home_relative(r"~\docs"), Some("docs"));
        assert_eq!(home_relative("~other"), None);
        assert_eq!(home_relative("docs"), None);
    }

    #[test]
    fn the_rest_joins_the_home_with_the_home_separator() {
        assert_eq!(under_home("/home/u", ""), "/home/u");
        assert_eq!(under_home("/home/u", "docs"), "/home/u/docs");
        assert_eq!(under_home(r"C:\Users\u", "docs"), r"C:\Users\u\docs");
    }

    #[test]
    fn a_new_connection_forgets_the_old_home_and_only_its_own_reply_counts() {
        let mut store = ExplorerViewStore::default();
        store.begin_home_probe(7, 1);
        assert!(store.finish_home_probe(1, Some("/home/a")));
        assert_eq!(store.remote_homes.get(&7), Some(&PathBuf::from("/home/a")));

        store.begin_home_probe(7, 2);
        assert_eq!(store.remote_homes.get(&7), None);
        assert!(!store.finish_home_probe(1, Some("/home/stale")));
        assert!(store.finish_home_probe(2, None));
        assert_eq!(store.remote_homes.get(&7), None);
        assert!(!store.finish_home_probe(2, Some("/home/again")));
    }

    #[test]
    fn the_address_bar_of_a_mirror_explorer_sees_its_workspace_home() {
        let mut store = ExplorerViewStore::default();
        let panel = tasty_model::ExplorerPanel::new(1, PathBuf::from("/remote/project"));
        store.begin_home_probe(7, 1);
        assert!(store.finish_home_probe(1, Some("/home/r")));

        let view = store.get_or_init(&panel, Some(7));
        assert!(matches!(
            view.address_host(true),
            Host::Remote { home: Some(h) } if h == std::path::Path::new("/home/r")
        ));
        let view = store.get_or_init(&panel, Some(8));
        assert!(matches!(
            view.address_host(true),
            Host::Remote { home: None }
        ));
        let view = store.get_or_init(&panel, None);
        assert!(matches!(view.address_host(false), Host::Local));
    }
}
