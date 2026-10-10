//! 탐색기 목록 키 가운데 선택에 하는 동작. 고른 항목을 열고, 선택을 푼다.

use super::{DirEntryInfo, ExplorerView};
use crate::adapters::ui::surface::explorer::ExplorerAction;

/// 항목을 여는 액션. 폴더는 그 폴더로 들어가고 파일은 연다. 더블클릭과 열기 키가 같이 쓴다.
pub(crate) fn open_action(entry: &DirEntryInfo) -> ExplorerAction {
    let path = entry.path.clone();
    if entry.is_dir {
        ExplorerAction::Navigate(path)
    } else {
        ExplorerAction::OpenFile(path)
    }
}

impl ExplorerView {
    /// 열기 키의 액션들. 지금 목록에 보이는 고른 항목만 센다(Find 로 걸러진 항목은 뺀다).
    /// 파일만 골랐으면 목록 순서대로 하나씩 연다. 폴더가 섞여 있으면 폴더가 하나일 때만 그
    /// 폴더로 들어가고 함께 고른 파일은 열지 않는다. 폴더가 둘 이상이면 아무것도 하지 않는다.
    pub(crate) fn open_selected_actions(&self) -> Vec<ExplorerAction> {
        let picked: Vec<&DirEntryInfo> = self
            .shown()
            .filter(|e| self.selected.contains(&e.path))
            .collect();
        let mut folders = picked.iter().filter(|e| e.is_dir);
        match (folders.next(), folders.next()) {
            (None, _) => picked.into_iter().map(open_action).collect(),
            (Some(folder), None) => vec![open_action(folder)],
            (Some(_), Some(_)) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn entry(name: &str, is_dir: bool) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from("/w").join(name),
            name: name.to_string(),
            is_dir,
            ..Default::default()
        }
    }

    fn view(entries: Vec<DirEntryInfo>) -> ExplorerView {
        let mut view = ExplorerView::new();
        view.set_entries(entries);
        view
    }

    fn opened(v: &ExplorerView) -> Vec<String> {
        v.open_selected_actions()
            .into_iter()
            .map(|a| match a {
                ExplorerAction::Navigate(p) => format!("go {}", p.display()),
                ExplorerAction::OpenFile(p) => format!("open {}", p.display()),
                _ => "other".to_string(),
            })
            .collect()
    }

    #[test]
    fn a_folder_is_entered_and_a_file_is_opened() {
        let mut v = view(vec![entry("docs", true), entry("a.txt", false)]);
        v.select_only(Path::new("/w/docs"));
        assert_eq!(opened(&v), ["go /w/docs"]);
        v.select_only(Path::new("/w/a.txt"));
        assert_eq!(opened(&v), ["open /w/a.txt"]);
    }

    /// 문서의 여러 항목 규칙: 파일만이면 모두 열고, 폴더 하나가 섞이면 그 폴더로만 가고, 폴더가
    /// 둘 이상이면 아무것도 하지 않는다.
    #[test]
    fn several_items_open_by_the_folder_count() {
        let mut v = view(vec![
            entry("docs", true),
            entry("src", true),
            entry("a.txt", false),
            entry("b.txt", false),
        ]);
        v.select_only(Path::new("/w/b.txt"));
        v.toggle_select(Path::new("/w/a.txt"));
        assert_eq!(opened(&v), ["open /w/a.txt", "open /w/b.txt"]);
        v.toggle_select(Path::new("/w/docs"));
        assert_eq!(opened(&v), ["go /w/docs"]);
        v.toggle_select(Path::new("/w/src"));
        assert!(opened(&v).is_empty());
    }

    #[test]
    fn nothing_opens_without_a_listed_selection() {
        let mut v = view(vec![entry("a.txt", false), entry("b.txt", false)]);
        assert!(opened(&v).is_empty());
        v.select_only(Path::new("/w/gone.txt"));
        assert!(opened(&v).is_empty());
    }

    /// 문서의 기준 항목 규칙: 전체 선택은 마지막 항목, 선택 해제는 없음.
    #[test]
    fn select_all_anchors_the_last_item_and_clearing_drops_the_anchor() {
        let mut v = view(vec![
            entry("a", false),
            entry("b", false),
            entry("c", false),
        ]);
        v.select_only(Path::new("/w/a"));
        v.select_all();
        assert_eq!(v.anchor.as_deref(), Some(Path::new("/w/c")));
        v.clear_selection();
        assert!(v.selected.is_empty());
        assert_eq!(v.anchor, None);
    }

    /// 숨김 파일을 끄면 숨겨지는 기준 항목과 선택이 빠지고, 다시 켜도 돌아오지 않는다. 보이는
    /// 기준 항목은 두 번 모두 그대로다.
    #[test]
    fn hiding_drops_a_hidden_anchor_and_showing_keeps_the_rest() {
        let mut v = view(vec![entry(".env", false), entry("a", false)]);
        v.toggle_hidden();
        v.select_all();
        v.anchor = Some(PathBuf::from("/w/.env"));
        v.toggle_hidden();
        assert_eq!(v.anchor, None);
        assert_eq!(v.selected.len(), 1);
        v.toggle_hidden();
        assert_eq!(v.anchor, None);
        assert_eq!(v.selected.len(), 1);

        v.select_only(Path::new("/w/a"));
        v.toggle_hidden();
        assert_eq!(v.anchor.as_deref(), Some(Path::new("/w/a")));
        v.toggle_hidden();
        assert_eq!(v.anchor.as_deref(), Some(Path::new("/w/a")));
    }
}
