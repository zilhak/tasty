//! "들어 있는 폴더에서 보기". 하위 폴더 검색 결과와 링크에서 항목이 실제로 든 폴더를 열고,
//! 그 목록이 들어오면 항목을 고르고 보이는 곳으로 스크롤한다. 깜박임 같은 강조는 없다.

use std::path::{Path, PathBuf};

use super::view::ExplorerView;

impl ExplorerView {
    /// 메뉴 행의 대상: (열 폴더, 고를 항목). 링크는 대상을 따라가 대상이 든 폴더를, 하위 폴더 검색
    /// 결과는 결과가 든 폴더를 연다. 그 밖의 항목과 대상이 없는 링크는 `None` 이다.
    pub(crate) fn enclosing_target(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
        let item = if is_link {
            std::fs::canonicalize(path).ok()?
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
            let link = tmp.path().join("link.md");
            std::os::unix::fs::symlink(&target, &link).expect("symlink");
            let real = std::fs::canonicalize(&real).expect("canonical");
            assert_eq!(
                view.enclosing_target(&link),
                Some((real.clone(), real.join("notes.md")))
            );
            let broken = tmp.path().join("broken.md");
            std::os::unix::fs::symlink(tmp.path().join("gone"), &broken).expect("symlink");
            assert_eq!(view.enclosing_target(&broken), None);
        }
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
