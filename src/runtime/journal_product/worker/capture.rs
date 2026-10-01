//! Persist immutable runtime captures on the storage worker, before deciding their reference facts.
use super::*;
use crate::runtime::journal_product::{EngineBinding,StreamCommand};
use crate::runtime::surface_capture::CapturedSurface;

pub(super) fn persist(executor:&Executor<StructureDecider>,ticket:u64,binding:EngineBinding,captures:Vec<CapturedSurface>)->Result<ResultValue,String> {
    executor.with_state(|_|()).map_err(|error|error.to_string())?;
    use sha2::{Digest,Sha256};
    let mut hash=Sha256::new();
    hash.update(b"tasty-surface-capture-v1");
    let header=serde_json::to_vec(&binding).map_err(|error|error.to_string())?;
    hash.update((header.len() as u64).to_le_bytes());hash.update(&header);
    for capture in &captures {
        let header=serde_json::to_vec(&(capture.surface,&capture.kind,capture.activation)).map_err(|error|error.to_string())?;
        hash.update((header.len() as u64).to_le_bytes());hash.update(&header);
        hash.update((capture.bytes.len() as u64).to_le_bytes());hash.update(&capture.bytes);
    }
    let digest=hash.finalize().to_vec();
    let (key,changes)={
        let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
        let epoch=inner.epoch;
        let key=tasty_event_store::CommandKey {caller_scope:"surface-capture".into(),idempotency_key:format!("{}/{ticket}",epoch.0)};
        match inner.store.lookup_command(&key,&digest).map_err(|error|error.to_string())? {
            CommandLookup::Hit(record)=>return Ok(ResultValue::Stored(record)),
            CommandLookup::DigestMismatch(_)=>return Err("capture identity conflicts with prior request".into()),
            CommandLookup::Miss=>{},
        }
        if binding.journal_id!=inner.store.journal_id() || binding.runtime_epoch!=epoch.0 {
            return Err("capture belongs to another journal runtime".into());
        }
        let model=inner.state.streams.get(&binding.stream).ok_or("capture stream missing")?;
        if model.engine_retired || model.engine_incarnation!=binding.incarnation {return Err("capture belongs to a retired engine".into());}
        // Removed/replaced instances are stale observations, never a reason to write to their successors.
        let captures:Vec<_>=captures.into_iter().filter(|capture|model.surfaces.get(&capture.surface).is_some_and(|surface|surface.kind==capture.kind && surface.activation.map(|activation|activation.generation)==capture.activation)).collect();
        let high_water=inner.state.streams.values().flat_map(|model|model.surfaces.values()).map(|surface|surface.content_generation).max().unwrap_or(0);
        let next=inner.store.next_unreserved_id("capture").map_err(|error|error.to_string())?;
        if next<=high_water {inner.store.reserve_ids(epoch,"capture",high_water-next+1,i64::MAX as u64).map_err(|error|error.to_string())?;}
        let mut changes=Vec::new();
        for capture in captures {
            let generation=inner.store.reserve_ids(epoch,"capture",1,i64::MAX as u64).map_err(|error|error.to_string())?.start;
            let reference=inner.store.put_payload(epoch,&capture.bytes).map_err(|error|error.to_string())?;
            changes.push(StreamCommand {stream:binding.stream.clone(),command:tasty_domain::StructuralCommand::RecordCapture {
                surface:capture.surface,kind:capture.kind,activation:capture.activation,content_generation:generation,snapshot_schema:1,data:tasty_domain::DataRef(reference.0),
            }});
        }
        (key,changes)
    };
    executor.execute(&ExecuteRequest {
        key:Some(key),actor:"system".into(),origin:"surface-capture".into(),causation_id:None,
        command:ResolvedCommand {original_digest:digest,response:None,changes,effect_result:None,cancellation:None,completion_view:None,original_results:Default::default()},
    }).map(ResultValue::Executed).map_err(|error|error.to_string())
}

/// A user close snapshots the canonical removed subtree. Lazy payload/seed references are copied
/// without opening a factory, while live observations replace only the matching captured instances.
pub(super) fn closed(executor:&Executor<StructureDecider>,ticket:u64,binding:EngineBinding,target:tasty_domain::CloseTarget,display_name:Option<String>,captures:Vec<CapturedSurface>)->Result<(tasty_domain::DataRef,Option<tasty_domain::UndoCapture>),String> {
    executor.with_state(|_|()).map_err(|error|error.to_string())?;
    let mut inner=executor.inner.lock().map_err(|error|error.to_string())?;
    let epoch=inner.epoch;
    let holder=format!("admission/{}/{ticket}",epoch.0);
    if binding.journal_id!=inner.store.journal_id() || binding.runtime_epoch!=epoch.0 {return Err("close capture journal runtime changed".into());}
    let model=inner.state.streams.get(&binding.stream).ok_or("close capture stream missing")?;
    if model.engine_retired || model.engine_incarnation!=binding.incarnation {return Err("close capture engine is retired".into());}
    let snapshot=tasty_domain::ClosedSnapshot::capture(model,target);
    let input=serde_json::to_vec(&target).map_err(|error|error.to_string())?;
    let input=inner.store.put_payload_pinned(epoch,&input,&holder).map_err(|error|error.to_string())?;
    let Some(mut snapshot)=snapshot else {return Ok((tasty_domain::DataRef(input.0),None));};
    for capture in captures {
        let surface=snapshot.surfaces.get_mut(&capture.surface).ok_or("capture is outside the closed subtree")?;
        if surface.kind!=capture.kind || surface.activation.map(|activation|activation.generation)!=capture.activation {
            return Err("closed snapshot observed an obsolete kind instance".into());
        }
        let reference=inner.store.put_payload_pinned(epoch,&capture.bytes,&holder).map_err(|error|error.to_string())?;
        surface.data=Some(tasty_domain::DataRef(reference.0));
        surface.snapshot_schema=1;
    }
    if snapshot.root.kind==tasty_domain::IdKind::Surface {snapshot.tab_name=display_name.or(snapshot.tab_name);}
    let retained=snapshot.data_refs();
    let bytes=serde_json::to_vec(&snapshot).map_err(|error|error.to_string())?;
    let reference=inner.store.put_payload_pinned(epoch,&bytes,&holder).map_err(|error|error.to_string())?;
    Ok((tasty_domain::DataRef(input.0),Some(tasty_domain::UndoCapture {snapshot:tasty_domain::DataRef(reference.0),retained})))
}
