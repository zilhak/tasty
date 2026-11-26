//! Folder mode target and the footer shared with gallery specimens.
use super::{FilePickerMode, FilePickerProps, FpViewState, join_dir};
use tasty_ui_widgets::FolderPickerFooter;

/// 폴더 모드의 확정 대상. 고른 하위 폴더가 있으면 그 폴더, 없으면 지금 보고 있는 폴더다.
pub(super) fn target(d: &crate::state::FilePickerData) -> String {
    match d.selected.as_slice() {
        [name] if d.entries.iter().any(|e| &e.name == name && e.is_dir) => {
            join_dir(d.mirror_ws_id.is_some(), &d.current_dir, name)
        }
        _ => d.current_dir.clone(),
    }
}

pub(super) fn footer<'a>(p: &'a FilePickerProps<'a>) -> Option<FolderPickerFooter<'a>> {
    let FilePickerMode::Folder { path, hint, .. } = p.mode else {
        return None;
    };
    Some(FolderPickerFooter {
        label: p.name_field_label,
        path,
        hint,
        selected: p.selected.first().map(String::as_str),
        confirm: p.confirm_label,
        cancel: p.cancel_label,
        ready: matches!(p.state, FpViewState::Loaded | FpViewState::Empty),
    })
}
