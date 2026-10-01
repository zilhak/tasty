//! Internal source-journal import into an explicitly selected fresh destination slot.
//! New IDs, copied payload refs, initial facts, first response and restore manifest commit together.
use super::*;

pub(crate) struct JournalImport<'a> {
    pub source: &'a mut EventStore,
    pub source_epoch: WriterEpoch,
    pub model: &'a JournalModel,
    pub view: &'a ImportedView,
    pub source_holder: &'a str,
    pub slot: LayoutSlotId,
    pub key: CommandKey,
    pub digest: Vec<u8>,
}

pub(crate) fn import(
    destination: &mut EventStore,
    epoch: WriterEpoch,
    request: JournalImport<'_>,
) -> Result<ImportOutcome, ImportError> {
    let holder = format!("admission/{}/journal-import/{}", epoch.0, request.key.idempotency_key);
    match destination.lookup_command(&request.key, &request.digest)? {
        CommandLookup::Hit(record) => {
            release_preparation(destination,epoch,&holder);
            return outcome(record);
        },
        CommandLookup::DigestMismatch(_) => return Err(ImportError::AlreadyImportedDifferently(request.slot)),
        CommandLookup::Miss => {},
    }
    let mut models = journal::load_all(destination)?;
    let stream = journal::engine_stream(request.slot);
    let current = models.stream(stream.as_str());
    if current.applied.revision.is_some() {
        return Err(StoreError::Corrupt("journal import destination slot is already initialized".into()).into());
    }
    let exported = crate::runtime::journal_payload::legacy_export::capture(request.source, request.model, request.view, false)
        .map_err(|error| StoreError::Corrupt(error))?;
    let layout = exported.layout;
    let categories = categories_of(&layout);
    // A different journal namespace is not permission to reuse numeric IDs still present in
    // shared metadata or referenced by a source cleanup. Reservation gaps remain intentional.
    for (kind, visible_max) in [
        (IdKind::Category,request.model.categories.keys().copied().max().unwrap_or(0)),
        (IdKind::Workspace,request.model.workspaces.keys().copied().max().unwrap_or(0)),
        (IdKind::Pane,request.model.panes.keys().copied().max().unwrap_or(0)),
        (IdKind::Tab,request.model.tabs.keys().copied().max().unwrap_or(0)),
        (IdKind::Surface,request.model.surfaces.keys().copied().max().unwrap_or(0)),
    ] {
        let source_next = request.source.next_unreserved_id(kind.label())?.max(u64::from(visible_max)+1);
        let next = destination.next_unreserved_id(kind.label())?;
        if next < source_next {destination.reserve_ids(epoch,kind.label(),source_next-next,max_id(kind))?;}
    }
    let ids = IdPools::reserve(destination, epoch, &layout, &categories)?;
    let references: Vec<_> = request.model.surfaces.values()
        .flat_map(|surface| surface.data.into_iter().chain(surface.creation_seed))
        .map(|reference| PayloadRef(reference.0)).collect();
    let copies = destination.import_payloads(epoch, request.source, request.source_epoch, &references, request.source_holder, &holder)?;
    let copies: BTreeMap<_, _> = copies.into_iter().map(|(source,destination)| (source.0,DataRef(destination.0))).collect();
    let mut plan = Plan::new(ids, &current);
    plan.push(DomainEvent::EngineIncarnationStarted {previous: 0, current: 1});
    plan.categories(&categories, &current)?;
    struct NoScrollback;
    impl ScrollbackSource for NoScrollback {
        fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> { Ok(None) }
    }
    let mut sink = Sink {store: destination, epoch, scrollback: &NoScrollback, holder: holder.clone()};
    for (index, workspace) in layout.workspaces.iter().enumerate() {plan.workspace(&mut sink,index,workspace)?;}
    let mapping = ImportMapping {slot:request.slot,categories:plan.category_map.clone(),workspaces:plan.workspaces.clone()};
    let surface_map = enrich(&mut plan, &mapping, request.model, &copies, destination, epoch)?;
    let mut view = view_of(&layout, &mapping);
    // Preserve the separate source View selection, including its existing ID-based surface subset.
    for (old_tab, old_surface) in &request.view.selected_surfaces {
        if let Some((new_tab, new_surface)) = surface_map.get(old_surface)
            && request.model.surfaces.get(old_surface).is_some_and(|surface| surface.tab == *old_tab) {
            view.selected_surfaces.insert(*new_tab,*new_surface);
        }
    }
    let command_id = format!("journal-import-{}-{}",epoch.0,request.key.idempotency_key);
    let commit = commit_request(epoch,&command_id,request.key,&request.digest,&mapping,&view,&plan,destination,stream.clone())?;
    let journal_id = destination.journal_id().to_owned();
    let restore_key = format!("view:slot-{}",request.slot);
    let result = destination.commit_with_restore_checkpoint(&commit, |batch| {
        journal::apply_all(&mut models,batch).map_err(|error| StoreError::Corrupt(error.to_string()))?;
        let model = models.streams.get(stream.as_str()).ok_or_else(|| StoreError::Corrupt("imported stream missing".into()))?;
        let binding = crate::runtime::journal_product::EngineBinding {
            journal_id,stream:stream.0.clone(),incarnation:model.engine_incarnation,runtime_epoch:epoch.0,
            published_cut:model.applied.batch,revision:model.applied.revision,
        };
        let saved = crate::runtime::journal_product::view_record::StoredView::imported(binding,view.clone());
        Ok((tasty_event_store::NewSnapshot {
            batch_id:batch.cut.batch_id,model_version:tasty_core::MODEL_VERSION,
            bytes:tasty_core::encode_snapshot(&models).map_err(|error| StoreError::Corrupt(error.to_string()))?,
            referenced_payloads:models.streams.values().flat_map(JournalModel::data_refs).map(|reference| PayloadRef(reference.0)).collect(),
        },tasty_event_store::NewRestoreManifest {
            restore_key,incarnation:1,runtime_epoch:epoch.0,sequence:0,snapshot_id:0,
            view:serde_json::to_vec(&saved).map_err(|error| StoreError::Corrupt(error.to_string()))?,referenced_payloads:Vec::new(),
        }))
    })?;
    release_preparation(destination,epoch,&holder);
    match result {
        CommitOutcome::Committed {batch} => Ok(ImportOutcome {mapping,view,batch:batch.map(|batch|batch.batch_id),missing_scrollback:Vec::new(),moved_to_normal:plan.moved_to_normal}),
        CommitOutcome::Duplicate(record) => outcome(record),
    }
}

pub(crate) fn outcome(record: tasty_event_store::CommandRecord) -> Result<ImportOutcome, ImportError> {
    let mapping = serde_json::from_slice(&record.resolved).map_err(ImportError::Mapping)?;
    let view = serde_json::from_slice(record.response.as_deref().ok_or_else(|| StoreError::Corrupt("import completion has no original View mapping".into()))?).map_err(ImportError::Mapping)?;
    Ok(ImportOutcome {mapping,view,batch:None,missing_scrollback:Vec::new(),moved_to_normal:Vec::new()})
}

fn enrich(
    plan: &mut Plan,
    mapping: &ImportMapping,
    source: &JournalModel,
    copies: &BTreeMap<u64,DataRef>,
    destination: &mut EventStore,
    epoch: WriterEpoch,
) -> Result<BTreeMap<u32,(u32,u32)>,ImportError> {
    let mut surfaces = BTreeMap::new();
    for (old_workspace,new_workspace) in source.workspace_order.iter().zip(&mapping.workspaces) {
        let workspace = &source.workspaces[old_workspace];
        for (key,value) in &workspace.metadata {plan.push(DomainEvent::MetadataSet {target:tasty_core::MetadataTarget::Workspace(new_workspace.id),key:key.clone(),value:value.clone()});}
        for (old_pane,new_pane) in workspace.layout.leaves().iter().zip(&new_workspace.panes) {
            for (old_tab,new_tab) in source.panes[old_pane].tabs.iter().zip(&new_pane.tabs) {
                for (old_surface,new_surface) in source.tabs[old_tab].layout.leaves().iter().zip(&new_tab.surfaces) {
                    surfaces.insert(*old_surface,(new_tab.id,*new_surface));
                }
            }
        }
    }
    let reverse: BTreeMap<_,_> = surfaces.iter().map(|(old,(_,new))| (*new,*old)).collect();
    for (event,pins) in &mut plan.events {
        let spec = match event {DomainEvent::TabCreated {surface,..}|DomainEvent::SurfaceSplit {surface,..} => surface,_=>continue};
        let old = &source.surfaces[&reverse[&spec.id]];
        spec.data = old.data.map(|reference| copies[&reference.0]);
        *pins = spec.data.into_iter().map(|reference|PayloadRef(reference.0)).collect();
    }
    for (old,(_,new)) in &surfaces {
        let surface = &source.surfaces[old];
        if let Some(seed) = surface.creation_seed {
            let input = copies[&seed.0];
            plan.events.push((DomainEvent::SurfaceSeedImported {id:*new,input},vec![PayloadRef(input.0)]));
        }
        if let Some(data) = surface.data {
            let data = copies[&data.0];
            let generation = destination.reserve_ids(epoch,"capture",1,i64::MAX as u64)?.start;
            plan.events.push((DomainEvent::SurfaceDataRecorded {id:*new,activation_generation:None,content_generation:generation,snapshot_schema:surface.snapshot_schema.max(1),data},vec![PayloadRef(data.0)]));
        }
        for (key,value) in &surface.metadata {plan.push(DomainEvent::MetadataSet {target:tasty_core::MetadataTarget::Surface(*new),key:key.clone(),value:value.clone()});}
    }
    Ok(surfaces)
}

fn release_preparation(destination:&mut EventStore,epoch:WriterEpoch,holder:&str) {
    if let Err(error)=destination.release_payload_holder(epoch,holder) {
        tracing::warn!(%error,"journal import committed; admission pin cleanup deferred");
    }
}
