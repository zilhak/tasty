//! Editable value projection. Queued edits contain no disk store or execution handle.
use std::collections::BTreeMap;
use tasty_presets::{LayoutPreset, PanePreset, PresetError, PresetKind, PresetResult, TabPreset, WorkspacePreset};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PresetValue { Workspace(WorkspacePreset), Tab(TabPreset), Pane(PanePreset) }
impl PresetValue {
    pub(crate) fn kind(&self) -> PresetKind { match self { Self::Workspace(_) => PresetKind::Workspace, Self::Tab(_) => PresetKind::Tab, Self::Pane(_) => PresetKind::Pane } }
    pub(crate) fn name(&self) -> &str { match self { Self::Workspace(p) => &p.name, Self::Tab(p) => &p.name, Self::Pane(p) => &p.name } }
    fn rename(&mut self, name: &str) { match self { Self::Workspace(p) => p.name = name.into(), Self::Tab(p) => p.name = name.into(), Self::Pane(p) => p.name = name.into() } }
}
#[derive(Clone, Debug)]
pub(crate) enum PresetEdit {
    Save { before: Option<PresetValue>, after: PresetValue },
    Delete { before: PresetValue },
    Rename { before: PresetValue, to: String },
}
#[derive(Default)]
pub(crate) struct PresetDrafts {
    workspaces: BTreeMap<String, WorkspacePreset>,
    tabs: BTreeMap<String, TabPreset>,
    panes: BTreeMap<String, PanePreset>,
    edits: Vec<PresetEdit>,
}
impl PresetDrafts {
    pub(crate) fn from_values(values: impl IntoIterator<Item = PresetValue>) -> Self {
        let mut draft = Self::default();
        for value in values { draft.insert(value); }
        draft
    }
    pub(crate) fn has_edits(&self) -> bool { !self.edits.is_empty() }
    pub(crate) fn take_edits(&mut self) -> Vec<PresetEdit> { std::mem::take(&mut self.edits) }
    pub(crate) fn list(&self, kind: PresetKind) -> Vec<String> { match kind {
        PresetKind::Workspace => self.workspaces.keys().cloned().collect(),
        PresetKind::Tab => self.tabs.keys().cloned().collect(),
        PresetKind::Pane => self.panes.keys().cloned().collect(),
    } }
    pub(crate) fn get_workspace(&self, name: &str) -> Option<&WorkspacePreset> { self.workspaces.get(name) }
    pub(crate) fn get_tab(&self, name: &str) -> Option<&TabPreset> { self.tabs.get(name) }
    pub(crate) fn get_pane(&self, name: &str) -> Option<&PanePreset> { self.panes.get(name) }
    fn get(&self, kind: PresetKind, name: &str) -> Option<PresetValue> { match kind {
        PresetKind::Workspace => self.get_workspace(name).cloned().map(PresetValue::Workspace),
        PresetKind::Tab => self.get_tab(name).cloned().map(PresetValue::Tab),
        PresetKind::Pane => self.get_pane(name).cloned().map(PresetValue::Pane),
    } }
    fn insert(&mut self, value: PresetValue) { match value {
        PresetValue::Workspace(p) => { self.workspaces.insert(p.name.clone(), p); }
        PresetValue::Tab(p) => { self.tabs.insert(p.name.clone(), p); }
        PresetValue::Pane(p) => { self.panes.insert(p.name.clone(), p); }
    } }
    fn remove(&mut self, kind: PresetKind, name: &str) { match kind {
        PresetKind::Workspace => { self.workspaces.remove(name); }
        PresetKind::Tab => { self.tabs.remove(name); }
        PresetKind::Pane => { self.panes.remove(name); }
    } }
    fn queue_save(&mut self, after: PresetValue, overwrite: bool) -> PresetResult<()> {
        validate_name(after.name())?;
        let before = self.get(after.kind(), after.name());
        if !overwrite && before.is_some() { return Err(PresetError::AlreadyExists(after.kind(), after.name().into())); }
        self.insert(after.clone());
        self.edits.push(PresetEdit::Save { before, after });
        Ok(())
    }
    pub(crate) fn queue_workspace(&mut self, mut value: WorkspacePreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Workspace(value), false)
    }
    pub(crate) fn queue_tab(&mut self, mut value: TabPreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Tab(value), false)
    }
    pub(crate) fn queue_pane(&mut self, mut value: PanePreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Pane(value), false)
    }
    pub(crate) fn queue_workspace_overwrite(&mut self, mut value: WorkspacePreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Workspace(value), true)
    }
    pub(crate) fn queue_tab_overwrite(&mut self, mut value: TabPreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Tab(value), true)
    }
    pub(crate) fn queue_pane_overwrite(&mut self, mut value: PanePreset) -> PresetResult<()> {
        value.normalize_surface_ids(); self.queue_save(PresetValue::Pane(value), true)
    }
    pub(crate) fn queue_delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()> {
        let before = self.get(kind, name).ok_or_else(|| PresetError::NotFound(kind, name.into()))?;
        self.remove(kind, name); self.edits.push(PresetEdit::Delete { before }); Ok(())
    }
    pub(crate) fn queue_rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()> {
        validate_name(to)?;
        let before = self.get(kind, from).ok_or_else(|| PresetError::NotFound(kind, from.into()))?;
        if from == to { return Ok(()); }
        if self.get(kind, to).is_some() { return Err(PresetError::AlreadyExists(kind, to.into())); }
        let mut next = before.clone(); next.rename(to);
        self.remove(kind, from); self.insert(next);
        self.edits.push(PresetEdit::Rename { before, to: to.into() }); Ok(())
    }
    pub(crate) fn unique_name(&self, kind: PresetKind, base: &str) -> String {
        tasty_presets::storage::unique_name_for(kind, base, |name| self.get(kind, name).is_some())
    }
}

use tasty_presets::storage::validate_name;
