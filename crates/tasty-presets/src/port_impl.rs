//! `impl PresetStorage for PresetStore` — 기존 inherent method delegation.

use crate::model::{PanePreset, PresetKind, TabPreset, WorkspacePreset};
use crate::port::PresetStorage;
use crate::storage::{PresetResult, PresetStore};

impl PresetStorage for PresetStore {
    fn list(&self, kind: PresetKind) -> Vec<String> {
        PresetStore::list(self, kind)
    }

    fn get_workspace(&self, name: &str) -> Option<&WorkspacePreset> {
        PresetStore::get_workspace(self, name)
    }

    fn get_tab(&self, name: &str) -> Option<&TabPreset> {
        PresetStore::get_tab(self, name)
    }

    fn get_pane(&self, name: &str) -> Option<&PanePreset> {
        PresetStore::get_pane(self, name)
    }

    fn save_workspace(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        PresetStore::save_workspace(self, preset)
    }

    fn save_tab(&mut self, preset: TabPreset) -> PresetResult<()> {
        PresetStore::save_tab(self, preset)
    }

    fn save_pane(&mut self, preset: PanePreset) -> PresetResult<()> {
        PresetStore::save_pane(self, preset)
    }

    fn save_workspace_overwrite(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        PresetStore::save_workspace_overwrite(self, preset)
    }

    fn save_tab_overwrite(&mut self, preset: TabPreset) -> PresetResult<()> {
        PresetStore::save_tab_overwrite(self, preset)
    }

    fn save_pane_overwrite(&mut self, preset: PanePreset) -> PresetResult<()> {
        PresetStore::save_pane_overwrite(self, preset)
    }

    fn delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()> {
        PresetStore::delete(self, kind, name)
    }

    fn rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()> {
        PresetStore::rename(self, kind, from, to)
    }

    fn unique_name(&self, kind: PresetKind, base: &str) -> String {
        PresetStore::unique_name(self, kind, base)
    }
}
