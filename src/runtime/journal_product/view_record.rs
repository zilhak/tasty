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

/// 저장된 View 선택을 읽는다. 저장소 자체를 읽지 못하면 `Err`다. 원본 바이트를 해석하거나 검증하지
/// 못하면 그 바이트를 보존한 뒤 선택 없이(기본 선택) 계속하고 그 사실을 함께 돌려준다.
pub(super) fn load(
    store: &tasty_event_store::EventStore,
    home: &Path,
    binding: &EngineBinding,
) -> Result<(Option<ImportedView>, Option<super::BindingIssue>), String> {
    match load_checked(store, home, binding)? {
        Ok(selection) => Ok((selection, None)),
        Err(unreadable) => {
            let preserved = unreadable.preserve(home, binding)?;
            tracing::warn!(
                stream = binding.stream,
                preserved,
                "saved View selection could not be read; using the default selection: {}",
                unreadable.cause
            );
            let slot =
                slot(binding)?.ok_or("headless engines do not own a persistent View slot")?;
            Ok((
                None,
                Some(super::BindingIssue::ViewUnreadable {
                    slot,
                    preserved,
                    cause: unreadable.cause,
                }),
            ))
        }
    }
}

/// 해석·검증하지 못한 View 원본.
struct Unreadable {
    cause: String,
    source: UnreadableSource,
}

enum UnreadableSource {
    /// DB restore manifest. 다음 저장이 같은 key를 바꾸므로 바이트를 따로 보존한다.
    Manifest { view: Vec<u8>, snapshot: Vec<u8> },
    /// legacy sidecar. 저장은 이 파일을 바꾸지 않는다. 읽은 바이트가 있으면 사본도 남긴다.
    Legacy {
        path: PathBuf,
        bytes: Option<Vec<u8>>,
    },
}

impl Unreadable {
    fn manifest(cause: impl Into<String>, manifest: &tasty_event_store::RestoreManifest) -> Self {
        Self {
            cause: cause.into(),
            source: UnreadableSource::Manifest {
                view: manifest.view.clone(),
                snapshot: manifest.snapshot.bytes.clone(),
            },
        }
    }

    /// 보존 위치를 돌려준다. 같은 바이트는 한 번만 쓰며, 이미 있는 보존 사본은 바꾸지 않는다.
    fn preserve(&self, home: &Path, binding: &EngineBinding) -> Result<String, String> {
        let name = format!(
            "view-{}-incarnation-{}",
            binding.stream.replace(':', "-"),
            binding.incarnation
        );
        let files: Vec<(String, &[u8])> = match &self.source {
            UnreadableSource::Legacy { path, bytes: None } => {
                return Ok(path.display().to_string());
            }
            UnreadableSource::Legacy {
                bytes: Some(bytes), ..
            } => vec![(format!("{name}-{}.legacy.json", digest(bytes)), bytes)],
            UnreadableSource::Manifest { view, snapshot } => {
                let id = digest(view);
                vec![
                    (format!("{name}-{id}.view"), view),
                    (format!("{name}-{id}.snapshot"), snapshot),
                ]
            }
        };
        let directory = damaged_directory(home);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        for (file, bytes) in &files {
            let path = directory.join(file);
            if path.exists() {
                continue;
            }
            let mut temporary =
                tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
            std::io::Write::write_all(&mut temporary, bytes).map_err(|error| error.to_string())?;
            temporary
                .as_file()
                .sync_all()
                .map_err(|error| error.to_string())?;
            temporary
                .persist_noclobber(&path)
                .map_err(|error| error.to_string())?;
        }
        Ok(directory.join(&files[0].0).display().to_string())
    }
}

/// 읽지 못한 View 원본의 보존 위치.
pub(crate) fn damaged_directory(home: &Path) -> PathBuf {
    home.join("structure/damaged")
}

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn load_checked(
    store: &tasty_event_store::EventStore,
    home: &Path,
    binding: &EngineBinding,
) -> Result<Result<Option<ImportedView>, Unreadable>, String> {
    let Some(key) = restore_key(binding)? else {
        return Ok(Ok(None));
    };
    if let Some(incarnation) = store
        .restore_manifest_incarnation(&key)
        .map_err(|error| error.to_string())?
    {
        if incarnation != binding.incarnation {
            return Ok(Ok(None));
        }
        let manifest = match store.restore_manifest(&key) {
            Ok(manifest) => manifest.ok_or("View manifest disappeared during worker read")?,
            // 이 manifest의 본문만 읽지 못한 것이다. 저장소의 다른 오류는 그대로 올린다.
            Err(
                error @ (tasty_event_store::StoreError::PayloadCorrupt(_)
                | tasty_event_store::StoreError::PayloadMissing(_)),
            ) => {
                let raw = store
                    .restore_manifest_raw(&key)
                    .map_err(|error| error.to_string())?
                    .ok_or("View manifest disappeared during worker read")?;
                return Ok(Err(Unreadable {
                    cause: error.to_string(),
                    source: UnreadableSource::Manifest {
                        view: raw.view.unwrap_or_default(),
                        snapshot: raw.snapshot.unwrap_or_default(),
                    },
                }));
            }
            Err(error) => return Err(error.to_string()),
        };
        return Ok(check_manifest(&manifest, binding)
            .map_err(|cause| Unreadable::manifest(cause, &manifest)));
    }
    // Only absence permits initial legacy import. Once a DB source exists, corrupt bytes never
    // fall back to an older sidecar. The first successful save establishes the durable DB source.
    let Some(path) = path(home, binding)? else {
        return Ok(Ok(None));
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Ok(None)),
        Err(error) => {
            return Ok(Err(Unreadable {
                cause: error.to_string(),
                source: UnreadableSource::Legacy { path, bytes: None },
            }));
        }
    };
    Ok(serde_json::from_slice(&bytes)
        .map_err(|error| error.to_string())
        .and_then(|saved| validate(saved, binding))
        .map_err(|cause| Unreadable {
            cause,
            source: UnreadableSource::Legacy {
                path,
                bytes: Some(bytes),
            },
        }))
}

fn check_manifest(
    manifest: &tasty_event_store::RestoreManifest,
    binding: &EngineBinding,
) -> Result<Option<ImportedView>, String> {
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
    validate(saved, binding)
}

fn slot(binding: &EngineBinding) -> Result<Option<u32>, String> {
    binding
        .stream
        .strip_prefix("structure:slot-")
        .map(|slot| slot.parse::<u32>().map_err(|error| error.to_string()))
        .transpose()
}

#[cfg(feature = "gui")]
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
        && let Some(previous) = read_legacy(home, &view.binding).unwrap_or_else(|error| {
            // 읽지 못한 sidecar는 열 때 이미 기본 선택으로 대체했다. 저장은 이 파일을 바꾸지 않는다.
            tracing::warn!("unreadable legacy View sidecar is left unchanged: {error}");
            None
        })
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
