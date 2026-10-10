//! "들어 있는 폴더에서 보기". 하위 폴더 검색 결과와 링크에서 항목이 실제로 든 폴더를 열고,
//! 그 목록이 들어오면 항목을 고르고 보이는 곳으로 스크롤한다. 깜박임 같은 강조는 없다.

use std::path::{Component, Path, PathBuf};

use super::view::ExplorerView;

impl ExplorerView {
    /// 메뉴 행의 대상: (열 폴더, 고를 항목). 링크는 대상이 든 폴더를, 하위 폴더 검색 결과는 결과가 든
    /// 폴더를 연다. 그 밖의 항목과 대상이 없는 링크는 `None` 이다.
    pub(crate) fn enclosing_target(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
        let item = if is_link {
            path.exists().then(|| link_target(path)).flatten()?
        } else if self.search_root().is_some() {
            path.to_path_buf()
        } else {
            return None;
        };
        Some((item.parent()?.to_path_buf(), item))
    }

    /// 폴더를 옮긴 뒤 부른다. Find 를 닫아 그 폴더의 전체 목록을 보이고, 목록에 `item` 이 나타나면 고른다.
    pub(crate) fn reveal_after_load(&mut self, item: PathBuf) {
        self.find = None;
        self.reveal = Some(item);
        self.reveal_select = true;
    }
}

/// 링크 자신만 한 번 푼 대상 경로. 상대 대상은 링크가 든 폴더에 이어 붙이고 `.`·`..` 만 글자로
/// 정리한다. 상위 경로의 링크는 풀지 않아 사용자가 보던 경로 체계를 지킨다. 대상이 다시 링크면
/// 그 링크가 대상이다(체인을 끝까지 따라가지 않는다).
fn link_target(link: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(link).ok()?;
    let joined = link.parent()?.join(target);
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fs_list::DirEntryInfo;

    fn listed(view: &mut ExplorerView, dir: &Path, names: &[&str]) {
        view.entries = names
            .iter()
            .map(|n| DirEntryInfo {
                path: dir.join(n),
                name: (*n).into(),
                is_dir: false,
                size: 0,
                modified: None,
                ext: String::new(),
                link: Default::default(),
            })
            .collect();
    }

    #[test]
    fn only_links_and_search_results_have_an_enclosing_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = tmp.path().join("real");
        std::fs::create_dir(&real).expect("mkdir");
        let target = real.join("notes.md");
        std::fs::write(&target, b"n").expect("write");
        let plain = tmp.path().join("plain.txt");
        std::fs::write(&plain, b"p").expect("write");
        let view = ExplorerView::default();
        assert_eq!(view.enclosing_target(&plain), None);
        #[cfg(unix)]
        {
            let broken = tmp.path().join("broken.md");
            std::os::unix::fs::symlink(tmp.path().join("gone"), &broken).expect("symlink");
            assert_eq!(view.enclosing_target(&broken), None);
        }
    }

    /// 링크를 만든 뒤 `enclosing_target` 이 돌려준 (폴더, 항목).
    #[cfg(unix)]
    fn target_of(link: &Path, to: impl AsRef<Path>) -> Option<(PathBuf, PathBuf)> {
        std::os::unix::fs::symlink(to, link).expect("symlink");
        ExplorerView::default().enclosing_target(link)
    }

    #[cfg(unix)]
    #[test]
    fn a_link_opens_its_target_folder_without_resolving_the_path_above() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = tmp.path().join("store");
        std::fs::create_dir_all(real.join("real/deep")).expect("mkdir");
        std::fs::write(real.join("real/notes.md"), b"n").expect("write");
        // 사용자는 링크된 폴더 `proj`(→ store) 안에서 본다. 결과는 `proj` 아래 경로여야 한다.
        let proj = tmp.path().join("proj");
        std::os::unix::fs::symlink(&real, &proj).expect("symlink");
        let seen = proj.join("real");

        let rel = target_of(&proj.join("rel.md"), "real/./deep/../notes.md");
        assert_eq!(rel, Some((seen.clone(), seen.join("notes.md"))));

        let up = target_of(&seen.join("deep/up.md"), "../notes.md");
        assert_eq!(up, Some((seen.clone(), seen.join("notes.md"))));

        let abs = target_of(&proj.join("abs.md"), proj.join("real/notes.md"));
        assert_eq!(abs, Some((seen.clone(), seen.join("notes.md"))));

        // 체인은 한 단계만 푼다: chain → rel.md(링크) 이면 rel.md 가 든 폴더에서 rel.md 를 고른다.
        let chain = target_of(&seen.join("chain.md"), "../rel.md");
        assert_eq!(chain, Some((proj.clone(), proj.join("rel.md"))));
    }

    #[test]
    fn the_item_is_selected_once_the_folder_lists_it() {
        let dir = Path::new("/srv/real");
        let item = dir.join("notes.md");
        let mut view = ExplorerView::default();
        view.reveal_after_load(item.clone());
        assert_eq!(view.take_reveal(), None, "not listed yet");
        assert!(view.selected.is_empty());
        listed(&mut view, dir, &["a.txt", "notes.md"]);
        assert_eq!(view.take_reveal(), Some(item.clone()));
        assert_eq!(view.selected.len(), 1);
        assert!(view.selected.contains(&item));
        assert_eq!(view.take_reveal(), None, "revealed once");
    }
}
