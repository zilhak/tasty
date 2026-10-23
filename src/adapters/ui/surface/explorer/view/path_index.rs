//! 경로순으로 정렬한 표시 번호를 재사용해 스크롤 대상의 전체 순회를 없앤다.

use std::cell::RefCell;
use std::path::Path;

use super::ExplorerView;

#[derive(Default)]
pub(crate) struct PathIndex {
    key: Option<(u64, usize, usize, String)>,
    /// 경로순 표시 번호. 경로 문자열은 복제하지 않는다.
    sorted: Vec<usize>,
    #[cfg(test)]
    rebuilds: usize,
}

impl PathIndex {
    pub(crate) fn clear(&mut self) {
        self.key = None;
        self.sorted.clear();
    }
}

impl ExplorerView {
    /// 목록 세대·검색어·표시 원본이 바뀔 때만 인덱스를 다시 만든다.
    pub(crate) fn shown_position(&self, path: &Path) -> Option<usize> {
        if self.shown_slice().is_none() && self.filtered().is_none() {
            // 필터 캐시가 준비되기 전에도 현재 검색어의 결과를 사용한다.
            return self.shown().position(|entry| entry.path == path);
        }
        let source = self
            .shown_slice()
            .map_or(self.entries.as_ptr(), |s| s.as_ptr()) as usize;
        let count = self.shown_count();
        let query = self.find_query_key();
        let mut index = self.path_index.borrow_mut();
        if !index
            .key
            .as_ref()
            .is_some_and(|(generation, address, len, q)| {
                *generation == self.entries_gen && *address == source && *len == count && q == query
            })
        {
            #[cfg(test)]
            {
                index.rebuilds += 1;
            }
            index.sorted = (0..count).collect();
            index.sorted.sort_unstable_by(|&a, &b| {
                self.shown_at(a)
                    .unwrap()
                    .path
                    .cmp(&self.shown_at(b).unwrap().path)
            });
            index.key = Some((self.entries_gen, source, count, query.to_owned()));
        }
        index
            .sorted
            .binary_search_by(|&i| self.shown_at(i).unwrap().path.as_path().cmp(path))
            .ok()
            .map(|i| index.sorted[i])
    }
}

pub(super) fn empty() -> RefCell<PathIndex> {
    RefCell::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fs_list::DirEntryInfo;
    use tasty_model::ExplorerPanel;

    fn entry(i: usize) -> DirEntryInfo {
        DirEntryInfo {
            path: format!("/index/item{i:06}.txt").into(),
            name: format!("item{i:06}.txt"),
            ..Default::default()
        }
    }

    #[test]
    fn the_large_index_is_reused_and_rebuilt_for_same_length_replacements() {
        let mut view = ExplorerView::new();
        view.set_entries((0..100_000).map(entry).collect());
        for _ in 0..8 {
            assert_eq!(view.shown_position(&entry(99_999).path), Some(99_999));
            assert_eq!(view.shown_position(Path::new("/missing")), None);
        }
        assert_eq!(view.path_index.borrow().rebuilds, 1);
        view.set_entries((0..100_000).rev().map(entry).collect());
        assert_eq!(view.shown_position(&entry(99_999).path), Some(0));
        assert_eq!(view.path_index.borrow().rebuilds, 2);
    }

    #[test]
    fn filtering_and_switching_tabs_never_reuse_another_display_order() {
        let mut panel = ExplorerPanel::new(1, "/index".into());
        let mut view = ExplorerView::new();
        view.sync(&panel, None);
        view.set_entries((0..10).map(entry).collect());
        view.open_find();
        view.find.as_mut().unwrap().set_query("item000009");
        view.refresh_filter();
        assert_eq!(view.shown_position(&entry(9).path), Some(0));
        assert_eq!(view.shown_position(&entry(8).path), None);
        view.find.as_mut().unwrap().set_query("item000008");
        view.refresh_filter();
        assert_eq!(view.shown_position(&entry(8).path), Some(0));
        assert_eq!(view.shown_position(&entry(9).path), None);
        panel.add_tab();
        view.sync(&panel, None);
        view.set_entries((0..10).rev().map(entry).collect());
        assert_eq!(view.shown_position(&entry(8).path), Some(1));
        panel.active = 0;
        view.sync(&panel, None);
        assert_eq!(view.shown_position(&entry(8).path), Some(0));
        assert_eq!(view.shown_position(&entry(9).path), None);
    }
}
