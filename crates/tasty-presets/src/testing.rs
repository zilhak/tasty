//! In-memory PresetStorage — test 시 disk 우회.
//!
//! `PresetStore::load_from(tempdir)` 도 가능하지만 *순수 in-memory* 가 더 빠르고
//! disk 사이드이펙트 없음.

use std::collections::HashMap;

use crate::model::{PanePreset, PresetKind, TabPreset, WorkspacePreset};
use crate::port::PresetStorage;
use crate::storage::PresetResult;

#[derive(Debug, Default)]
pub struct InMemoryPresetStorage {
    workspace: HashMap<String, WorkspacePreset>,
    tab: HashMap<String, TabPreset>,
    pane: HashMap<String, PanePreset>,
}

impl InMemoryPresetStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PresetStorage for InMemoryPresetStorage {
    fn list(&self, kind: PresetKind) -> Vec<String> {
        let names: Vec<String> = match kind {
            PresetKind::Workspace => self.workspace.keys().cloned().collect(),
            PresetKind::Tab => self.tab.keys().cloned().collect(),
            PresetKind::Pane => self.pane.keys().cloned().collect(),
        };
        let mut names = names;
        names.sort();
        names
    }

    fn get_workspace(&self, name: &str) -> Option<&WorkspacePreset> {
        self.workspace.get(name)
    }
    fn get_tab(&self, name: &str) -> Option<&TabPreset> {
        self.tab.get(name)
    }
    fn get_pane(&self, name: &str) -> Option<&PanePreset> {
        self.pane.get(name)
    }

    fn save_workspace(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        self.workspace.insert(preset.name.clone(), preset);
        Ok(())
    }
    fn save_tab(&mut self, preset: TabPreset) -> PresetResult<()> {
        self.tab.insert(preset.name.clone(), preset);
        Ok(())
    }
    fn save_pane(&mut self, preset: PanePreset) -> PresetResult<()> {
        self.pane.insert(preset.name.clone(), preset);
        Ok(())
    }

    fn save_workspace_overwrite(&mut self, preset: WorkspacePreset) -> PresetResult<()> {
        self.save_workspace(preset)
    }
    fn save_tab_overwrite(&mut self, preset: TabPreset) -> PresetResult<()> {
        self.save_tab(preset)
    }
    fn save_pane_overwrite(&mut self, preset: PanePreset) -> PresetResult<()> {
        self.save_pane(preset)
    }

    fn delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()> {
        match kind {
            PresetKind::Workspace => {
                self.workspace.remove(name);
            }
            PresetKind::Tab => {
                self.tab.remove(name);
            }
            PresetKind::Pane => {
                self.pane.remove(name);
            }
        }
        Ok(())
    }

    fn rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()> {
        match kind {
            PresetKind::Workspace => {
                if let Some(mut p) = self.workspace.remove(from) {
                    p.name = to.to_string();
                    self.workspace.insert(to.to_string(), p);
                }
            }
            PresetKind::Tab => {
                if let Some(mut p) = self.tab.remove(from) {
                    p.name = to.to_string();
                    self.tab.insert(to.to_string(), p);
                }
            }
            PresetKind::Pane => {
                if let Some(mut p) = self.pane.remove(from) {
                    p.name = to.to_string();
                    self.pane.insert(to.to_string(), p);
                }
            }
        }
        Ok(())
    }

    fn unique_name(&self, kind: PresetKind, base: &str) -> String {
        let exists = |n: &str| -> bool {
            match kind {
                PresetKind::Workspace => self.workspace.contains_key(n),
                PresetKind::Tab => self.tab.contains_key(n),
                PresetKind::Pane => self.pane.contains_key(n),
            }
        };
        if !exists(base) {
            return base.to_string();
        }
        for i in 2..u32::MAX {
            let candidate = format!("{base} ({i})");
            if !exists(&candidate) {
                return candidate;
            }
        }
        base.to_string()
    }
}
