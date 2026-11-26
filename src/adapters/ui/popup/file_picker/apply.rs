//! 피커 동작 하나를 상태에 반영한다. 닫힘·확정은 팝업 동작으로 돌려준다.
use super::*;

/// 동작 하나를 반영한다. 확정·닫힘이면 그 팝업 동작을, 아니면 `PopupAction::None` 을 돌려준다.
pub(super) fn apply_action(
    d: &mut crate::state::FilePickerData,
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    action: FilePickerAction,
) -> PopupAction {
    match action {
        FilePickerAction::None => PopupAction::None,
        FilePickerAction::ClearSelection => {
            d.selected.clear();
            d.row_focus = crate::state::FpRowFocus::default();
            PopupAction::None
        }
        FilePickerAction::HideRing => {
            d.row_focus.ring = false;
            PopupAction::None
        }
        FilePickerAction::ShowRing { scroll } => {
            d.row_focus.show_ring(scroll);
            PopupAction::None
        }
        FilePickerAction::FocusRow {
            name,
            select,
            scroll,
        } => {
            if select {
                d.selected = vec![name.clone()];
            }
            d.row_focus.place(name, true, Some(scroll));
            PopupAction::None
        }
        FilePickerAction::PickRow { name, scroll } => {
            let out = apply_action(d, state, engine, FilePickerAction::Select(name.clone()));
            d.row_focus.place(name, true, Some(scroll));
            out
        }
        FilePickerAction::Cancel => {
            d.result = Some(FilePickerResult::Cancelled);
            PopupAction::Close
        }
        FilePickerAction::EditName(name) => {
            if d.save_name.is_some() {
                if d.selected.first() != Some(&name) {
                    d.selected.clear();
                }
                d.save_name = Some(name);
            }
            PopupAction::None
        }
        FilePickerAction::Select(name) => {
            if d.save_name.is_some()
                && d.entries
                    .iter()
                    .any(|e| e.name == name && !e.is_dir && matches_filters(&d.filters, &e.name))
            {
                d.save_name = Some(name.clone());
            }
            // 행 클릭은 행 포커스를 그 행으로 옮기고 링을 숨긴다.
            d.row_focus.place(name.clone(), false, None);
            d.selected = vec![name];
            PopupAction::None
        }
        FilePickerAction::NavigateInto(name) => {
            navigate(d, state, engine, |dir, is_remote| {
                join_dir(is_remote, dir, &name)
            });
            PopupAction::None
        }
        FilePickerAction::NavigateTo(idx) => {
            let is_remote = d.mirror_ws_id.is_some();
            let targets = path_ancestors(is_remote, &d.current_dir);
            if let Some(target) = targets.get(idx).cloned() {
                navigate(d, state, engine, move |_dir, _is_remote| target.clone());
            }
            PopupAction::None
        }
        FilePickerAction::NavigateUp if !up_enabled(d, engine) => PopupAction::None,
        FilePickerAction::NavigateUp => {
            let is_remote = d.mirror_ws_id.is_some();
            let targets = path_ancestors(is_remote, &d.current_dir);
            if targets.len() > 1 {
                let parent = targets[targets.len() - 2].clone();
                navigate(d, state, engine, move |_dir, _is_remote| parent.clone());
            }
            PopupAction::None
        }
        FilePickerAction::Refresh => {
            navigate(d, state, engine, |dir, _is_remote| dir.to_string());
            PopupAction::None
        }
        FilePickerAction::Confirm => {
            if d.folder {
                if !matches!(d.load, FpLoadState::Loaded | FpLoadState::Empty) {
                    return PopupAction::None;
                }
                let path = folder::target(d);
                d.result = Some(FilePickerResult::Confirmed { paths: vec![path] });
                return PopupAction::Close;
            }
            if let Some(name) = &d.save_name {
                if let Some(path) = save::confirmed_path(
                    std::path::Path::new(&d.current_dir),
                    name,
                    matches!(d.load, FpLoadState::Loaded | FpLoadState::Empty),
                    &d.entries,
                ) {
                    d.result = Some(FilePickerResult::Confirmed { paths: vec![path] });
                    return PopupAction::Close;
                }
                return PopupAction::None;
            }
            // 열기 확정은 footer 버튼과 같이 목록이 Loaded 일 때만이다(세션 소실·재조회 중 옛 선택을 열지 않는다).
            if !matches!(d.load, FpLoadState::Loaded) {
                return PopupAction::None;
            }
            let folder = match d.selected.as_slice() {
                [name] => d
                    .entries
                    .iter()
                    .find(|e| &e.name == name)
                    .filter(|e| e.is_dir)
                    .map(|e| e.name.clone()),
                _ => None,
            };
            if let Some(name) = folder {
                navigate(d, state, engine, |dir, is_remote| {
                    join_dir(is_remote, dir, &name)
                });
                return PopupAction::None;
            }
            let is_remote = d.mirror_ws_id.is_some();
            // 화면의 활성화 검사와 별개로 디렉터리를 파일로 확정하지 않게 재확인한다.
            let all_files = d.selected.iter().all(|name| {
                d.entries
                    .iter()
                    .find(|e| &e.name == name)
                    .is_some_and(|e| !e.is_dir)
            });
            let paths: Vec<String> = d
                .selected
                .iter()
                .map(|name| join_dir(is_remote, &d.current_dir, name))
                .collect();
            if all_files && !paths.is_empty() {
                d.result = Some(FilePickerResult::Confirmed { paths });
                return PopupAction::Close;
            }
            PopupAction::None
        }
        FilePickerAction::ConfirmEntry(name) => {
            if d.folder || d.save_name.is_some() {
                return apply_action(d, state, engine, FilePickerAction::Select(name));
            }
            let is_remote = d.mirror_ws_id.is_some();
            let path = join_dir(is_remote, &d.current_dir, &name);
            d.result = Some(FilePickerResult::Confirmed { paths: vec![path] });
            PopupAction::Close
        }
    }
}
