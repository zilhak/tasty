//! Draft identity is checked both after edits and immediately before consuming a file result.
use super::SettingsUiState;
impl SettingsUiState {
    pub(crate) fn invalidate_edited_export(&mut self) {
        if let Some(draft) = self.draft.as_ref() {
            self.import_export.observe_export_draft(
                &draft.keybindings,
                &self.plugin_bundle,
                &self.plugin_shortcuts_draft,
            );
        }
    }
}
