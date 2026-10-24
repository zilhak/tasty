//! Preserve dormant View sources without retaining unrelated streams' capture history.

pub(super) fn rebase(
    store: &mut tasty_event_store::EventStore,
    epoch: tasty_event_store::WriterEpoch,
    models: &tasty_core::StructureModels,
    target: tasty_event_store::SnapshotId,
) -> Result<(), String> {
    for (key, incarnation) in store
        .restore_manifest_bindings()
        .map_err(|e| e.to_string())?
    {
        let Some(slot) = key
            .strip_prefix("view:slot-")
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let stream = format!("structure:slot-{slot}");
        let Some(current) = models.streams.get(&stream) else {
            continue;
        };
        if current.engine_retired || current.engine_incarnation != incarnation {
            continue;
        }
        let Some(manifest) = store.restore_manifest(&key).map_err(|e| e.to_string())? else {
            continue;
        };
        if manifest.snapshot.snapshot_id == target {
            continue;
        }
        let old =
            tasty_core::decode_snapshot(manifest.snapshot.model_version, &manifest.snapshot.bytes)
                .map_err(|e| e.to_string())?;
        let Some(mut previous) = old.streams.get(&stream).cloned() else {
            continue;
        };
        if old.batch != manifest.snapshot.cut.last_batch || previous.applied.batch != old.batch {
            return Err("restore manifest snapshot cut differs from its model".into());
        }
        // evolve_streams advances every model's global applied batch even for unrelated events.
        // Normalize only that bookkeeping cut; revision and every semantic field must still match.
        previous.applied.batch = current.applied.batch;
        if &previous != current {
            tracing::debug!(%key, "restore manifest retains its changed stream cut");
            continue;
        }
        let binding = super::super::EngineBinding {
            journal_id: store.journal_id().to_owned(),
            stream: stream.clone(),
            incarnation,
            runtime_epoch: manifest.runtime_epoch,
            published_cut: current.applied.batch,
            revision: current.applied.revision,
        };
        let _selection = super::super::view_record::check_manifest(&manifest, &binding)?;
        if store
            .rebase_restore_manifest(
                epoch,
                &key,
                &tasty_event_store::StreamId::new(stream),
                target,
            )
            .map_err(|e| e.to_string())?
        {
            tracing::debug!(%key, target, "rebased equivalent restore manifest");
        }
    }
    Ok(())
}
