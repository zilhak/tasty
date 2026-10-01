//! Apply committed facts to the live tree without recreating surviving kind instances.
//!
//! Prepared objects and retired objects each have one owner. Resource execution is the caller's
//! responsibility; this module neither spawns a process nor reads a stored payload.

pub(crate) mod bootstrap;
mod layout;
mod structure;
mod replacement;


use tasty_domain::{DomainBatch, DomainEvent, JournalModel};

use super::shadow_digest::{Canonical, SkipData, live};
use crate::core::CoreState;
use crate::model::{Pane, SurfaceDescriptor, Tab, Workspace};

pub(crate) enum Retired {
    Workspace(Workspace),
    Pane(Pane),
    Tab(Tab),
    Surface(SurfaceDescriptor),
}

type Result<T> = std::result::Result<T, String>;

/// A mismatch halts publication. The caller must not publish any engine in a partially applied batch.
/// Both comparisons exclude content payloads, resource status, and View selections.
pub(crate) fn apply(
    engine: &mut CoreState,
    before: &JournalModel,
    batch: &DomainBatch,
    retired: &mut Vec<Retired>,
) -> Result<()> {
    preflight(engine, before, batch)?;
    for recorded in &batch.events {
        structure::apply_event(engine, &recorded.event, retired)?;
    }
    let mut after = before.clone();
    tasty_domain::evolve(&mut after, batch).map_err(|error| error.to_string())?;
    if live::core_canonical(engine) != Canonical::of_journal(&after, &SkipData) {
        return Err("committed live projection differs from the canonical result".into());
    }
    engine.committed_structure_revision = after.applied.revision;
    for (id,surface) in &after.surfaces {
        if let Some(descriptor)=engine.find_surface_descriptor_mut(*id) {
            descriptor.activation_generation=surface.activation.map(|activation|activation.generation);
        }
    }
    Ok(())
}

fn preflight(engine:&CoreState,before:&JournalModel,batch:&DomainBatch)->Result<()> {
    if live::core_canonical(engine)!=Canonical::of_journal(before,&SkipData) {
        return Err("live projection is not at the command's committed predecessor".into());
    }
    let mut after=before.clone();
    tasty_domain::evolve(&mut after,batch).map_err(|error|error.to_string())?;
    Ok(())
}

fn workspace(engine: &mut CoreState, id: u32) -> Result<&mut Workspace> {
    engine
        .local_workspaces
        .iter_mut()
        .find(|workspace| workspace.id == id)
        .ok_or_else(|| format!("local workspace {id} is missing"))
}

fn pane(engine: &mut CoreState, id: u32) -> Result<&mut Pane> {
    engine
        .local_workspaces
        .iter_mut()
        .find_map(|workspace| workspace.pane_layout_mut().find_pane_mut(id))
        .ok_or_else(|| format!("local pane {id} is missing"))
}

fn tab(engine: &mut CoreState, id: u32) -> Result<&mut Tab> {
    engine
        .local_workspaces
        .iter_mut()
        .find_map(|workspace| layout::find_tab_mut(workspace.pane_layout_mut(), id))
        .ok_or_else(|| format!("local tab {id} is missing"))
}

#[cfg(test)]
mod tests;
