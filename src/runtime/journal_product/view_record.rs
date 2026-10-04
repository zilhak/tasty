//! View selection is a binding-scoped source in the journal restore manifest, separate from domain events.
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

pub(super) fn load(
    store: &tasty_event_store::EventStore,
    home: &Path,
    binding: &EngineBinding,
) -> Result<Option<ImportedView>, String> {
    let Some(key) = restore_key(binding)? else {
        return Ok(None);
    };
    if let Some(incarnation) = store
        .restore_manifest_incarnation(&key)
        .map_err(|error| error.to_string())?
    {
        if incarnation != binding.incarnation {
            return Ok(None);
        }
        let manifest = store
            .restore_manifest(&key)
            .map_err(|error| error.to_string())?
            .ok_or("View manifest disappeared during worker read")?;
        let models =
            tasty_core::decode_snapshot(manifest.snapshot.model_version, &manifest.snapshot.bytes)
                .map_err(|error| error.to_string())?;
        if models.batch != manifest.snapshot.cut.last_batch {
            return Err("View manifest domain snapshot cut differs".into());
        }
        let model = models
            .streams
            .get(&binding.stream)
            .ok_or("View manifest engine stream missing")?;
        if model.engine_incarnation != binding.incarnation || model.engine_retired {
            return Err("View manifest domain snapshot belongs to another incarnation".into());
        }
        let saved: StoredView =
            serde_json::from_slice(&manifest.view).map_err(|error| error.to_string())?;
        if (
            saved.binding.incarnation,
            saved.binding.runtime_epoch,
            saved.sequence,
        ) != (
            manifest.incarnation,
            manifest.runtime_epoch,
            manifest.sequence,
        ) {
            return Err("View manifest ordering differs from its source record".into());
        }
        if saved.binding.journal_id != binding.journal_id
            || saved.binding.stream != binding.stream
            || saved.binding.incarnation != binding.incarnation
            || saved.binding.revision > model.applied.revision
            || saved.binding.published_cut > model.applied.batch
        {
            return Err("View manifest source does not match its domain checkpoint".into());
        }
        return validate(saved, binding);
    }
    // Only absence permits initial legacy import. Once a DB source exists, corrupt bytes never
    // fall back to an older sidecar. The first successful save establishes the durable DB source.
    read_legacy(home, binding)?
        .map(|saved| validate(saved, binding))
        .transpose()
        .map(Option::flatten)
}

fn read_legacy(home: &Path, binding: &EngineBinding) -> Result<Option<StoredView>, String> {
    let Some(path) = path(home, binding)? else {
        return Ok(None);
    };
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn validate(saved: StoredView, binding: &EngineBinding) -> Result<Option<ImportedView>, String> {
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
pub(super) fn restore_key(binding: &EngineBinding) -> Result<Option<String>, String> {
    let Some(slot) = binding.stream.strip_prefix("structure:slot-") else {
        return Ok(None);
    };
    let slot = slot.parse::<u32>().map_err(|error| error.to_string())?;
    Ok(Some(format!("view:slot-{slot}")))
}

#[cfg(feature = "gui")]
pub(super) fn save(
    store: &mut tasty_event_store::EventStore,
    epoch: tasty_event_store::WriterEpoch,
    snapshot: &tasty_event_store::NewSnapshot,
    home: &Path,
    view: &StoredView,
) -> Result<(), String> {
    let key =
        restore_key(&view.binding)?.ok_or("headless engines do not own a persistent View slot")?;
    if store
        .restore_manifest_incarnation(&key)
        .map_err(|error| error.to_string())?
        .is_none()
        && let Some(previous) = read_legacy(home, &view.binding)?
    {
        if previous.version != 1 {
            return Err("unsupported legacy View snapshot version".into());
        }
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
    store
        .save_restore_checkpoint(
            epoch,
            snapshot,
            &tasty_event_store::NewRestoreManifest {
                restore_key: key,
                incarnation: view.binding.incarnation,
                runtime_epoch: view.binding.runtime_epoch,
                sequence: view.sequence,
                snapshot_id: 0,
                view: serde_json::to_vec(view).map_err(|error| error.to_string())?,
                referenced_payloads: Vec::new(),
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

impl StoredView {
    pub(crate) fn imported(binding: EngineBinding, selection: ImportedView) -> Self {
        Self {
            version: 1,
            binding,
            selection,
            sequence: 0,
        }
    }
    pub(crate) fn selection(&self) -> &ImportedView {
        &self.selection
    }
    pub(crate) fn validate_import(
        &self,
        journal: &str,
        model: &tasty_core::JournalModel,
    ) -> Result<(), String> {
        if self.version != 1
            || self.binding.journal_id != journal
            || self.binding.incarnation != model.engine_incarnation
            || model.engine_retired
            || self.binding.revision > model.applied.revision
            || self.binding.published_cut > model.applied.batch
        {
            return Err("source View manifest does not match its frozen domain".into());
        }
        Ok(())
    }
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
            core.local_workspaces(),
            core.categories(),
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
