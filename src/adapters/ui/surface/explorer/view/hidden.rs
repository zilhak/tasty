//! 숨김 파일 표시. 이름이 점으로 시작하는 항목(`..` 는 제외)과 OS 가 숨김으로 표시한 항목을 숨김 파일로 본다.
//! 끄면 목록(`entries`)에서 빼 따로 두고 상태줄에 그 수를 보인다. 켜면 되돌려 넣고 정렬 자리로 보낸다.
//! View 가 토글을 바로 바꿔 그리고, 바뀐 값은 `take_change` 로 model(`ExplorerPanel::show_hidden`)에
//! 남겨 레이아웃 스냅샷에 싣는다. model 값은 미리보기 패널처럼 마지막으로 맞춘 값과 다를 때만 받는다.

use std::path::Path;

use crate::core::fs_list::{DirEntryInfo, sort_entries};

use super::ExplorerView;

/// 숨김 파일인가. 이름이 점으로 시작하거나(`..` 행 제외) OS 가 숨김으로 표시했다
/// (Windows 숨김 속성·macOS `UF_HIDDEN`, `DirEntryInfo::os_hidden`).
pub(crate) fn is_hidden(entry: &DirEntryInfo) -> bool {
    entry.os_hidden || (entry.name.starts_with('.') && entry.name != "..")
}

/// 숨김 파일 표시 상태.
#[derive(Default)]
pub(crate) struct HiddenFiles {
    /// 숨김 파일을 목록에 보이는가. 기본은 끔이다.
    pub(crate) show: bool,
    /// 끈 동안 목록에서 뺀 숨김 항목. 켜면 목록으로 돌아간다.
    stash: Vec<DirEntryInfo>,
    /// model 과 마지막으로 맞춘 값. 처음 그릴 때는 `None` 이라 복원한 값을 받는다.
    known: Option<bool>,
    /// 사용자가 바꿔 model 에 아직 남기지 않았다.
    changed: bool,
}

impl HiddenFiles {
    /// 끈 동안 목록에서 뺀 숨김 항목 수.
    pub(crate) fn count(&self) -> usize {
        self.stash.len()
    }

    /// 목록을 통째로 비울 때 뺀 항목도 비운다.
    pub(super) fn forget(&mut self) {
        self.stash.clear();
    }

    /// 바뀐 값을 한 번 꺼낸다. model 이 곧 이 값이 되므로 다음 프레임에 다시 받지 않는다.
    pub(crate) fn take_change(&mut self) -> Option<bool> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        self.known = Some(self.show);
        Some(self.show)
    }
}

impl ExplorerView {
    /// 새 목록에서 숨김 항목을 뺀다. 켜져 있으면 그대로 둔다. `set_entries` 가 부른다.
    pub(super) fn split_hidden(&mut self, mut entries: Vec<DirEntryInfo>) -> Vec<DirEntryInfo> {
        self.hidden.stash.clear();
        if !self.hidden.show {
            let (hidden, shown): (Vec<_>, Vec<_>) = entries.into_iter().partition(is_hidden);
            self.hidden.stash = hidden;
            entries = shown;
        }
        entries
    }

    /// 사이드바 트리에 보일 `dir` 의 하위 폴더. 끈 동안에는 숨김 폴더를 뺀다.
    pub(super) fn tree_shown(&self, dir: &Path) -> Vec<&DirEntryInfo> {
        self.tree_children
            .get(dir)
            .into_iter()
            .flatten()
            .filter(|e| self.hidden.show || !is_hidden(e))
            .collect()
    }

    /// 끈 동안 목록에서 뺀 숨김 항목 이름. 새 항목 이름이 겹치는지 볼 때 함께 센다.
    pub(crate) fn hidden_names(&self) -> impl Iterator<Item = &str> {
        self.hidden.stash.iter().map(|e| e.name.as_str())
    }

    /// 숨김 파일을 끈 동안 뺀 항목이 있으면 "{n} items · {h} hidden" 처럼 수를 덧붙인다.
    pub(crate) fn hidden_status(&self, items: String) -> String {
        match self.hidden.count() {
            0 => items,
            n => format!(
                "{items} · {}",
                crate::i18n::t_count("explorer.status.hidden", n as u64, &[&n.to_string()])
            ),
        }
    }

    /// More 행·툴바·단축키의 토글. 바뀐 값은 렌더 뒤 model 에 남긴다.
    pub(crate) fn toggle_hidden(&mut self) {
        self.set_show_hidden(!self.hidden.show);
        self.hidden.changed = true;
    }

    /// model 값이 마지막으로 맞춘 값과 다를 때만 받는다. 처음 그릴 때는 레이아웃에서 복원한 값을 받는다.
    pub(super) fn adopt_hidden(&mut self, show: bool) {
        if self.hidden.known == Some(show) {
            return;
        }
        self.hidden.known = Some(show);
        self.hidden.changed = false;
        self.set_show_hidden(show);
    }

    /// 지금 목록과 뺀 항목을 합쳐 다시 나눈다. 켤 때는 정렬 자리로 넣고, 끌 때는 숨김 항목의 선택을 푼다.
    fn set_show_hidden(&mut self, show: bool) {
        if self.hidden.show == show {
            return;
        }
        self.hidden.show = show;
        let mut all = std::mem::take(&mut self.entries);
        all.append(&mut self.hidden.stash);
        if let Some((_, col, dir)) = &self.loaded {
            sort_entries(&mut all, *col, *dir);
        }
        self.set_entries(all);
        self.retain_listed_selection();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tasty_model::{SortColumn, SortDir};

    use super::*;

    fn entry(name: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from("/w").join(name),
            name: name.to_string(),
            is_dir: false,
            size: 0,
            modified: None,
            ext: String::new(),
            ..Default::default()
        }
    }

    fn names(view: &ExplorerView) -> Vec<&str> {
        view.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn dot_names_are_hidden_but_the_parent_row_is_not() {
        assert!(is_hidden(&entry(".env")));
        assert!(is_hidden(&entry(".git")));
        assert!(!is_hidden(&entry("..")));
        assert!(!is_hidden(&entry("notes.md")));
        assert!(!is_hidden(&entry("a.b")));
    }

    #[test]
    fn an_os_hidden_mark_hides_a_name_without_a_dot() {
        let marked = DirEntryInfo {
            os_hidden: true,
            ..entry("desktop.ini")
        };
        assert!(is_hidden(&marked));
        assert!(!is_hidden(&entry("desktop.ini")));
        let mut view = ExplorerView::new();
        view.set_entries(vec![marked, entry("a.txt")]);
        assert_eq!(names(&view), ["a.txt"]);
        assert_eq!(view.hidden.count(), 1);
    }

    #[test]
    fn hidden_items_leave_the_listing_until_shown_then_return_in_sort_order() {
        let mut view = ExplorerView::new();
        view.loaded = Some((PathBuf::from("/w"), SortColumn::Name, SortDir::Asc));
        view.set_entries(vec![
            entry(".env"),
            entry("b.txt"),
            entry(".git"),
            entry("a.txt"),
        ]);
        assert_eq!(names(&view), ["b.txt", "a.txt"]);
        assert_eq!(view.hidden.count(), 2);

        view.toggle_hidden();
        assert_eq!(names(&view), [".env", ".git", "a.txt", "b.txt"]);
        assert_eq!(view.hidden.count(), 0);
        assert_eq!(view.hidden.take_change(), Some(true));
        assert_eq!(view.hidden.take_change(), None);

        // 끄면 숨김 항목의 선택도 풀린다.
        view.selected.insert(PathBuf::from("/w/.env"));
        view.selected.insert(PathBuf::from("/w/a.txt"));
        view.toggle_hidden();
        assert_eq!(names(&view), ["a.txt", "b.txt"]);
        assert_eq!(view.hidden.count(), 2);
        assert!(view.selected.contains(&PathBuf::from("/w/a.txt")));
        assert!(!view.selected.contains(&PathBuf::from("/w/.env")));
    }

    #[test]
    fn the_sidebar_tree_follows_the_toggle_and_keeps_its_cache() {
        let mut view = ExplorerView::new();
        let dir = PathBuf::from("/w");
        view.tree_children
            .insert(dir.clone(), vec![entry(".git"), entry("src")]);
        let tree = |view: &ExplorerView| -> Vec<String> {
            view.tree_shown(&dir)
                .iter()
                .map(|e| e.name.clone())
                .collect()
        };
        assert_eq!(tree(&view), ["src"]);
        view.toggle_hidden();
        assert_eq!(tree(&view), [".git", "src"]);
        view.toggle_hidden();
        assert_eq!(tree(&view), ["src"]);
        assert_eq!(view.tree_children[&dir].len(), 2);
    }

    #[test]
    fn the_model_value_is_adopted_once_and_a_user_change_is_not_undone() {
        let mut view = ExplorerView::new();
        view.set_entries(vec![entry(".env"), entry("a.txt")]);
        // 복원한 값(켬)을 처음 그릴 때 받는다. 사용자 변경이 아니라 model 에 다시 남기지 않는다.
        view.adopt_hidden(true);
        assert!(view.hidden.show);
        assert_eq!(view.hidden.take_change(), None);
        // model 이 그대로면 사용자가 바꾼 값을 덮어쓰지 않는다.
        view.toggle_hidden();
        view.adopt_hidden(true);
        assert!(!view.hidden.show);
        assert_eq!(view.hidden.take_change(), Some(false));
        view.adopt_hidden(false);
        assert!(!view.hidden.show);
    }
}
