//! Latest View selection is an explicit binding-scoped sidecar, never a structure event or source.
use super::EngineBinding;
use crate::core::layout_persistence::import::ImportedView;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredView {
    version: u32,
    pub(crate) binding: EngineBinding,
    selection: ImportedView,
    pub(crate) sequence: u64,
}

fn path(home: &Path, binding: &EngineBinding) -> Result<Option<PathBuf>, String> {
    let Some(slot) = binding.stream.strip_prefix("structure:slot-") else {
        return Ok(None);
    };
    let slot = slot.parse::<u32>().map_err(|error| error.to_string())?;
    Ok(Some(
        home.join("structure/views")
            .join(format!("slot-{slot}"))
            .join(format!("incarnation-{}.json", binding.incarnation)),
    ))
}

pub(super) fn load(home: &Path, binding: &EngineBinding) -> Result<Option<ImportedView>, String> {
    let Some(path) = path(home, binding)? else {
        return Ok(None);
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let saved: StoredView = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if saved.version != 1 {
        return Err("unsupported View snapshot version".into());
    }
    if saved.binding.journal_id != binding.journal_id
        || saved.binding.stream != binding.stream
        || saved.binding.incarnation != binding.incarnation
    {
        return Ok(None);
    }
    if saved.binding.revision > binding.revision
        || saved.binding.published_cut > binding.published_cut
    {
        return Err("View checkpoint is ahead of its committed structure".into());
    }
    Ok(Some(saved.selection))
}

#[cfg(feature = "gui")]
pub(super) fn save(home: &Path, view: &StoredView) -> Result<(), String> {
    let Some(path) = path(home, &view.binding)? else {
        return Err("headless engines do not own a persistent View slot".into());
    };
    std::fs::create_dir_all(path.parent().expect("View directory"))
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    for parent in path
        .ancestors()
        .skip(1)
        .take_while(|parent| parent.starts_with(home))
    {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            let previous: StoredView =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if previous.binding.journal_id == view.binding.journal_id
                && previous.binding.stream == view.binding.stream
                && (
                    previous.binding.incarnation,
                    previous.binding.runtime_epoch,
                    previous.sequence,
                ) > (
                    view.binding.incarnation,
                    view.binding.runtime_epoch,
                    view.sequence,
                )
            {
                return Err("stale View checkpoint cannot replace a newer saved selection".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    super::identity::write_binding(&path, view)
}

#[cfg(feature = "gui")]
impl StoredView {
    pub(crate) fn capture(
        binding: EngineBinding,
        core: &crate::core::CoreState,
        active: Option<u32>,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Self {
        let captured = crate::model::StructurePresentationSnapshot::capture(
            &core.local_workspaces(),
            &core.categories(),
            presentation,
        );
        Self {
            version: 1,
            binding,
            sequence: 0,
            selection: ImportedView {
                active_workspace: active.filter(|id| {
                    core.local_workspaces()
                        .iter()
                        .any(|workspace| workspace.id == *id)
                }),
                focused_panes: captured.panes.into_iter().collect(),
                active_tabs: captured.selected_tabs.into_iter().collect(),
                selected_surfaces: captured.surfaces.into_iter().collect(),
                collapsed_categories: captured.collapsed_categories.into_iter().collect(),
            },
        }
    }
}
