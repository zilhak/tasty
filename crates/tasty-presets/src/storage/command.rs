//! Owned commands and cache changes; no borrowed preset or UI mutex crosses the worker boundary.
use super::{PresetKindAccess, PresetResult, PresetStore, unique_name_for, validate_name};
use crate::{LayoutPreset, PanePreset, PresetKind, TabPreset, WorkspacePreset};

#[derive(Debug, Clone, PartialEq)]
pub enum OwnedPreset {
    Workspace(WorkspacePreset),
    Tab(TabPreset),
    Pane(PanePreset),
}
impl OwnedPreset {
    pub fn kind(&self) -> PresetKind {
        match self {
            Self::Workspace(_) => PresetKind::Workspace,
            Self::Tab(_) => PresetKind::Tab,
            Self::Pane(_) => PresetKind::Pane,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Workspace(p) => &p.name,
            Self::Tab(p) => &p.name,
            Self::Pane(p) => &p.name,
        }
    }
    pub(super) fn set_name(&mut self, name: String) {
        match self {
            Self::Workspace(p) => p.name = name,
            Self::Tab(p) => p.name = name,
            Self::Pane(p) => p.name = name,
        }
    }
    pub(super) fn serialized(&self) -> PresetResult<String> {
        Ok(match self {
            Self::Workspace(p) => toml::to_string_pretty(p)?,
            Self::Tab(p) => toml::to_string_pretty(p)?,
            Self::Pane(p) => toml::to_string_pretty(p)?,
        })
    }
}

/// Apply to a UI cache only after a successful command receipt, in admission order.
#[derive(Debug, Clone)]
pub enum PresetChange {
    Saved(OwnedPreset),
    Removed { kind: PresetKind, name: String },
}
impl PresetStore {
    /// Blocking worker/standalone boundary. The root lock covers name choice and replacement.
    /// An explicit occupied name with overwrite=false is a successful no-op.
    pub fn save_command(
        &mut self,
        base: &str,
        explicit: Option<&str>,
        overwrite: bool,
        mut preset: OwnedPreset,
    ) -> PresetResult<Option<PresetChange>> {
        let _guard = self.mutation_lock()?;
        let kind = preset.kind();
        let dir = self.kind_dir(kind)?;
        let occupied = |name: &str| {
            self.contains_inner(kind, name)
                || std::fs::symlink_metadata(dir.join(format!("{name}.toml"))).is_ok()
        };
        let name = match explicit {
            Some(name) => {
                validate_name(name)?;
                if !overwrite && occupied(name) {
                    return Ok(None);
                }
                name.to_owned()
            }
            None => unique_name_for(kind, base, occupied),
        };
        preset.set_name(name.clone());
        match preset {
            OwnedPreset::Workspace(p) => self.save_inner_locked(p, overwrite)?,
            OwnedPreset::Tab(p) => self.save_inner_locked(p, overwrite)?,
            OwnedPreset::Pane(p) => self.save_inner_locked(p, overwrite)?,
        }
        let value = match kind {
            PresetKind::Workspace => OwnedPreset::Workspace(self.workspaces[&name].clone()),
            PresetKind::Tab => OwnedPreset::Tab(self.tabs[&name].clone()),
            PresetKind::Pane => OwnedPreset::Pane(self.panes[&name].clone()),
        };
        Ok(Some(PresetChange::Saved(value)))
    }

    /// Memory only: publishing a worker's committed value never performs disk I/O.
    pub fn apply_change(&mut self, change: PresetChange) {
        match change {
            PresetChange::Removed { kind, name } => self.remove_cached(kind, &name),
            PresetChange::Saved(OwnedPreset::Workspace(p)) => {
                WorkspacePreset::insert_into_store(self, p.name().into(), p)
            }
            PresetChange::Saved(OwnedPreset::Tab(p)) => {
                TabPreset::insert_into_store(self, p.name().into(), p)
            }
            PresetChange::Saved(OwnedPreset::Pane(p)) => {
                PanePreset::insert_into_store(self, p.name().into(), p)
            }
        }
    }
}
