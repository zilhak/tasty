//! 경로를 해시한 슬롯에 표시 번호만 보관해 경로 복제와 비교 정렬을 피한다.

use std::cell::RefCell;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::path::Path;

use super::ExplorerView;

#[derive(Default)]
pub(crate) struct PathIndex {
    key: Option<(u64, usize, usize, String)>,
    /// 빈 슬롯은 usize::MAX. 충돌은 다음 슬롯으로 넘기며 실제 경로를 비교해 조회한다.
    slots: Vec<usize>,
    hash: RandomState,
    #[cfg(test)]
    rebuilds: usize,
}

impl PathIndex {
    pub(crate) fn clear(&mut self) {
        self.key = None;
        self.slots.clear();
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
            // 절반 이하의 점유율로 선형 탐사 길이를 제한한다. 원본 경로는 빌리기만 한다.
            let capacity = count.saturating_mul(2).max(1).next_power_of_two();
            index.slots = vec![usize::MAX; capacity];
            let mask = capacity - 1;
            for (i, entry) in self.shown().enumerate() {
                let mut at = index.hash.hash_one(&entry.path) as usize & mask;
                while index.slots[at] != usize::MAX {
                    at = (at + 1) & mask;
                }
                index.slots[at] = i;
            }
            index.key = Some((self.entries_gen, source, count, query.to_owned()));
        }
        let mask = index.slots.len() - 1;
        let mut at = index.hash.hash_one(path) as usize & mask;
        loop {
            let i = index.slots[at];
            if i == usize::MAX {
                return None;
            }
            if self.shown_at(i).unwrap().path == path {
                return Some(i);
            }
            at = (at + 1) & mask;
        }
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
        let rebuilds_before_switch = view.path_index.borrow().rebuilds;
        panel.add_tab();
        view.sync(&panel, None);
        view.set_entries((0..10).rev().map(entry).collect());
        assert_eq!(view.shown_position(&entry(8).path), Some(1));
        panel.active = 0;
        view.sync(&panel, None);
        assert_eq!(view.shown_position(&entry(8).path), Some(0));
        assert_eq!(view.shown_position(&entry(9).path), None);
        assert_eq!(view.path_index.borrow().rebuilds, rebuilds_before_switch);
    }

    #[test]
    fn hash_collisions_compare_real_paths_and_probe_to_the_first_empty_slot() {
        let mut view = ExplorerView::new();
        let colliding: Vec<_> = (0..1000)
            .map(entry)
            .filter(|e| view.path_index.borrow().hash.hash_one(&e.path) & 7 == 0)
            .take(5)
            .collect();
        assert_eq!(colliding.len(), 5);
        view.set_entries(colliding[..4].to_vec());
        for (i, entry) in colliding[..4].iter().enumerate() {
            assert_eq!(view.shown_position(&entry.path), Some(i));
        }
        assert_eq!(view.shown_position(&colliding[4].path), None);
    }

    #[test]
    fn a_loaded_remote_sync_preserves_generation_and_index_until_sort_changes() {
        use tasty_model::SortDir;
        let mut panel = ExplorerPanel::new(1, "/index".into());
        let mut view = ExplorerView::new();
        view.sync(&panel, Some(42));
        let id = view.outbox[0].request_id;
        assert!(view.apply_remote_list_dir_result(id, &panel, Ok((0..10).map(entry).collect())));
        assert_eq!(view.shown_position(&entry(9).path), Some(9));
        let generation = view.entries_gen;
        let address = view.entries.as_ptr();
        for _ in 0..20 {
            view.sync(&panel, Some(42));
            assert_eq!(view.entries_gen, generation);
            assert_eq!(view.entries.as_ptr(), address);
            assert_eq!(view.shown_position(&entry(9).path), Some(9));
        }
        assert_eq!(view.path_index.borrow().rebuilds, 1);
        panel.active_tab_mut().sort_dir = SortDir::Desc;
        view.sync(&panel, Some(42));
        assert_ne!(view.entries_gen, generation);
        assert_eq!(view.shown_position(&entry(9).path), Some(0));
        assert_eq!(view.path_index.borrow().rebuilds, 2);
    }
}
