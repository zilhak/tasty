//! Destructive commands retain committed destination changes on a partial rename.
use super::{
    OwnedPreset, PresetChange, PresetError, PresetKind, PresetResult, PresetStore, validate_name,
};
use std::{io, path::Path};
use tasty_utils::file_replace;

#[derive(Debug)]
pub struct PresetMutationOutcome {
    /// Apply every committed change before publishing `result`, including a partial failure.
    pub changes: Vec<PresetChange>,
    pub result: PresetResult<()>,
}

impl PresetStore {
    pub(super) fn rename_cached(
        &mut self,
        kind: PresetKind,
        from: &str,
        to: &str,
        source_removed: bool,
    ) -> PresetResult<()> {
        fn change<P: crate::LayoutPreset + Clone>(
            map: &mut std::collections::BTreeMap<String, P>,
            kind: PresetKind,
            from: &str,
            to: &str,
            source_removed: bool,
        ) -> PresetResult<()> {
            if map.contains_key(to) {
                return Err(PresetError::AlreadyExists(kind, to.into()));
            }
            let mut value = if source_removed {
                map.remove(from)
            } else {
                map.get(from).cloned()
            }
            .ok_or_else(|| PresetError::NotFound(kind, from.into()))?;
            value.set_name(to.into());
            map.insert(to.into(), value);
            Ok(())
        }
        match kind {
            PresetKind::Workspace => change(&mut self.workspaces, kind, from, to, source_removed),
            PresetKind::Tab => change(&mut self.tabs, kind, from, to, source_removed),
            PresetKind::Pane => change(&mut self.panes, kind, from, to, source_removed),
        }
    }
    pub(super) fn remove_cached(&mut self, kind: PresetKind, name: &str) {
        match kind {
            PresetKind::Workspace => {
                self.workspaces.remove(name);
            }
            PresetKind::Tab => {
                self.tabs.remove(name);
            }
            PresetKind::Pane => {
                self.panes.remove(name);
            }
        }
    }
    fn owned(&self, kind: PresetKind, name: &str) -> Option<OwnedPreset> {
        match kind {
            PresetKind::Workspace => self
                .workspaces
                .get(name)
                .cloned()
                .map(OwnedPreset::Workspace),
            PresetKind::Tab => self.tabs.get(name).cloned().map(OwnedPreset::Tab),
            PresetKind::Pane => self.panes.get(name).cloned().map(OwnedPreset::Pane),
        }
    }
    pub fn delete(&mut self, kind: PresetKind, name: &str) -> PresetResult<()> {
        let _guard = self.mutation_lock()?;
        if !self.contains_inner(kind, name) {
            return Err(PresetError::NotFound(kind, name.into()));
        }
        let path = self.kind_dir(kind)?.join(format!("{name}.toml"));
        // A missing kind directory already achieves deletion; target canonicalization follows
        // metadata only when a document still exists.
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                super::observed::check_target(&path)?;
                let _file = file_replace::lock(&path)?;
                if let Err(error) = self.observed.check(&path) {
                    match std::fs::symlink_metadata(&path) {
                        Err(missing) if missing.kind() == io::ErrorKind::NotFound => {
                            self.observed.forget(&path);
                            self.remove_cached(kind, name);
                            return Ok(());
                        }
                        _ => return Err(error.into()),
                    }
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        self.observed.forget(&path);
        self.remove_cached(kind, name);
        Ok(())
    }
    pub fn delete_command(&mut self, kind: PresetKind, name: &str) -> PresetMutationOutcome {
        let result = self.delete(kind, name);
        let changes = if result.is_ok() {
            vec![PresetChange::Removed {
                kind,
                name: name.into(),
            }]
        } else {
            Vec::new()
        };
        PresetMutationOutcome { changes, result }
    }
    pub fn rename(&mut self, kind: PresetKind, from: &str, to: &str) -> PresetResult<()> {
        self.rename_with_remove(kind, from, to, |path| std::fs::remove_file(path))
    }
    /// The injectable final syscall lets tests fail cleanup after a real destination commit.
    pub(super) fn rename_with_remove(
        &mut self,
        kind: PresetKind,
        from: &str,
        to: &str,
        remove: impl FnOnce(&Path) -> io::Result<()>,
    ) -> PresetResult<()> {
        let _guard = self.mutation_lock()?;
        validate_name(to)?;
        let mut value = self
            .owned(kind, from)
            .ok_or_else(|| PresetError::NotFound(kind, from.into()))?;
        if from == to {
            return Ok(());
        }
        let dir = self.kind_dir(kind)?;
        let from_path = dir.join(format!("{from}.toml"));
        let to_path = dir.join(format!("{to}.toml"));
        super::observed::check_target(&from_path)?;
        super::observed::check_target(&to_path)?;
        if self.contains_inner(kind, to) {
            return Err(PresetError::AlreadyExists(kind, to.into()));
        }
        match std::fs::symlink_metadata(&to_path) {
            Ok(_) => return Err(PresetError::AlreadyExists(kind, to.into())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let _source = file_replace::lock(&from_path)?;
        self.observed.check(&from_path)?;
        value.set_name(to.into());
        self.observed
            .replace_from(&to_path, value.serialized()?.as_bytes(), Some(&from_path))?;
        self.apply_change(PresetChange::Saved(value))?;
        // An uncooperative editor can still change the source while the destination is written.
        // Preserve that edit and report the already committed destination rather than unlink it.
        let cleanup = self
            .observed
            .check(&from_path)
            .and_then(|()| match remove(&from_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            });
        if let Err(source) = cleanup {
            return Err(PresetError::RenamePartial {
                kind,
                from: from.into(),
                to: to.into(),
                source,
            });
        }
        self.observed.forget(&from_path);
        self.remove_cached(kind, from);
        Ok(())
    }
    pub fn rename_command(
        &mut self,
        kind: PresetKind,
        from: &str,
        to: &str,
    ) -> PresetMutationOutcome {
        let result = self.rename(kind, from, to);
        self.rename_outcome(kind, from, to, result)
    }
    pub(super) fn rename_outcome(
        &self,
        kind: PresetKind,
        from: &str,
        to: &str,
        result: PresetResult<()>,
    ) -> PresetMutationOutcome {
        let mut changes = Vec::new();
        if from != to
            && (result.is_ok() || matches!(&result, Err(PresetError::RenamePartial { .. })))
        {
            changes.push(PresetChange::Renamed {
                kind,
                from: from.into(),
                to: to.into(),
                source_removed: result.is_ok(),
            });
        }
        PresetMutationOutcome { changes, result }
    }
}
