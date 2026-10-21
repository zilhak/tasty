//! Folder-only keyboard actions and the footer shared with gallery specimens.
use super::{FilePickerAction, FilePickerMode, FilePickerProps, FpViewState};
use tasty_ui_widgets::folder_picker::FolderPickerFooter;

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

pub(super) fn keyboard(ui: &egui::Ui, p: &FilePickerProps<'_>) -> Option<FilePickerAction> {
    if ui.memory(|m| m.any_popup_open())
        || !p.owns_escape
        || !matches!(p.mode, FilePickerMode::Folder { .. })
    {
        return None;
    }
    let consume = |key, modifiers| ui.input_mut(|i| i.consume_key(modifiers, key));
    if consume(egui::Key::ArrowUp, egui::Modifiers::ALT) {
        return Some(FilePickerAction::NavigateUp);
    }
    if !matches!(p.state, FpViewState::Loaded | FpViewState::Empty) {
        return None;
    }
    if consume(egui::Key::Enter, egui::Modifiers::NONE) {
        return Some(FilePickerAction::Confirm);
    }
    let folders: Vec<_> = p
        .entries
        .iter()
        .filter(|e| e.is_dir)
        .map(|e| &e.name)
        .collect();
    let current = folders
        .iter()
        .position(|name| p.selected.first() == Some(*name));
    if consume(egui::Key::ArrowRight, egui::Modifiers::NONE) {
        return current.map(|i| FilePickerAction::NavigateInto(folders[i].clone()));
    }
    if folders.is_empty() {
        return None;
    }
    let next = if consume(egui::Key::ArrowDown, egui::Modifiers::NONE) {
        Some(current.map_or(0, |i| (i + 1).min(folders.len() - 1)))
    } else if consume(egui::Key::ArrowUp, egui::Modifiers::NONE) {
        Some(current.map_or(folders.len() - 1, |i| i.saturating_sub(1)))
    } else {
        None
    };
    next.filter(|i| Some(*i) != current)
        .map(|i| FilePickerAction::Select(folders[i].clone()))
}
