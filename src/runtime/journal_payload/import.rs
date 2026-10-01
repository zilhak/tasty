//! Explicit internal journal transfer. Normal startup still selects the data home's one journal.
use std::path::PathBuf;
use tasty_event_store::{CommandKey, CommandLookup, EventStore, NewRestoreManifest, WriterEpoch};
use crate::core::layout_persistence::import::{ImportOutcome, journal_import};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourceImport {
    pub(crate) source_path: PathBuf,
    pub(crate) source_journal: String,
    pub(crate) source_restore_key: String,
    pub(crate) destination_slot: u32,
    pub(crate) transfer_id: String,
}
impl SourceImport {
    pub(crate) fn weight(&self) -> usize {
        self.source_path.as_os_str().len().saturating_add(self.source_journal.len())
            .saturating_add(self.source_restore_key.len()).saturating_add(self.transfer_id.len()).saturating_add(128)
    }
}

pub(crate) fn transfer(destination: &mut EventStore, epoch: WriterEpoch, request: &SourceImport) -> Result<ImportOutcome, String> {
    if request.transfer_id.is_empty() || !request.source_path.is_absolute() {
        return Err("journal import requires an explicit source path and stable transfer identity".into());
    }
    if request.source_journal == destination.journal_id() {return Err("source journal import cannot target the same journal".into());}
    let key = CommandKey {caller_scope:"journal-import".into(),idempotency_key:request.transfer_id.clone()};
    let digest = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    let alias = format!("import-source:{}/{}", destination.journal_id(), request.transfer_id);
    match destination.lookup_command(&key,&digest).map_err(|error| error.to_string())? {
        CommandLookup::Hit(record) => {
            if let Err(error) = destination.release_payload_holder(epoch,&format!("admission/{}/journal-import/{}",epoch.0,request.transfer_id)) {
                tracing::warn!(%error,"completed import admission cleanup deferred");
            }
            // The destination is already complete even when the old source was moved, is busy, or
            // vanished after handoff. Never rerun the import to manufacture a new mapping.
            cleanup_source(request,&alias);
            return journal_import::outcome(record).map_err(|error| error.to_string());
        },
        CommandLookup::DigestMismatch(_) => return Err("journal transfer identity belongs to another request".into()),
        CommandLookup::Miss => {},
    }
    let mut source = open_source(request)?;
    let source_epoch = source.acquire_writer().map_err(|error| error.to_string())?;
    let frozen = match source.restore_manifest(&alias).map_err(|error| error.to_string())? {
        Some(frozen) => frozen,
        None => {
            let selected = source.restore_manifest(&request.source_restore_key).map_err(|error| error.to_string())?
                .ok_or("source restore manifest is missing")?;
            source.save_restore_manifest(source_epoch,&NewRestoreManifest {
                restore_key:alias.clone(),incarnation:selected.incarnation,runtime_epoch:selected.runtime_epoch,
                sequence:selected.sequence,snapshot_id:selected.snapshot.snapshot_id,view:selected.view,
                referenced_payloads:Vec::new(),
            }).map_err(|error| error.to_string())?;
            source.restore_manifest(&alias).map_err(|error| error.to_string())?.ok_or("source transfer pin missing")?
        },
    };
    let models = tasty_core::decode_snapshot(frozen.snapshot.model_version,&frozen.snapshot.bytes).map_err(|error| error.to_string())?;
    if models.batch != frozen.snapshot.cut.last_batch {return Err("source domain snapshot cut differs".into());}
    let view: crate::runtime::journal_product::view_record::StoredView = serde_json::from_slice(&frozen.view).map_err(|error| error.to_string())?;
    let model = models.streams.get(&view.binding.stream).ok_or("source restore stream missing")?;
    view.validate_import(source.journal_id(),model)?;
    let result = journal_import::import(destination,epoch,journal_import::JournalImport {
        source:&mut source,source_epoch,model,view:view.selection(),source_holder:&alias,
        slot:request.destination_slot,key,digest,
    }).map_err(|error| error.to_string())?;
    // Destination initial facts, ID mapping, first response, domain snapshot and View manifest are
    // committed before source ownership is released. No source effect/cleanup is retargeted.
    if let Err(error) = release_source(&mut source,source_epoch,&alias,frozen.incarnation) {
        tracing::warn!(%error,"journal transfer completed; source import pin retained for retry");
    }
    Ok(result)
}
fn open_source(request: &SourceImport) -> Result<EventStore,String> {
    if !request.source_path.is_file() {return Err("explicit source journal file is missing".into());}
    EventStore::open(&request.source_path,&request.source_journal).map_err(|error| error.to_string())
}
fn release_source(source:&mut EventStore,epoch:WriterEpoch,alias:&str,incarnation:u64)->Result<(),String> {
    // Payload and snapshot pins are separate holders; both remain conservative until release.
    source.release_payload_holder(epoch,alias).map_err(|error|error.to_string())?;
    source.delete_restore_manifest(epoch,alias,incarnation).map_err(|error|error.to_string())?;
    Ok(())
}
fn cleanup_source(request:&SourceImport,alias:&str) {
    let cleanup = ||->Result<(),String> {
        let mut source = open_source(request)?;
        let epoch = source.acquire_writer().map_err(|error|error.to_string())?;
        if let Some(incarnation)=source.restore_manifest_incarnation(alias).map_err(|error|error.to_string())? {
            release_source(&mut source,epoch,alias,incarnation)?;
        }
        Ok(())
    };
    if let Err(error)=cleanup() {tracing::warn!(%error,"completed import source cleanup deferred");}
}
