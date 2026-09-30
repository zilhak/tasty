//! Apply committed facts to the live tree without recreating surviving kind instances.
//!
//! Prepared objects and retired objects each have one owner. Resource execution is the caller's
//! responsibility; this module neither spawns a process nor reads a stored payload.

mod layout;
mod structure;

use std::collections::HashMap;

use tasty_domain::{DomainBatch, DomainEvent, JournalModel};

use super::shadow_digest::{Canonical, SkipData, live};
use crate::core::CoreState;
use crate::model::{Pane, Surface, Tab, Workspace};

pub(crate) enum Retired {
    Workspace(Workspace),
    Pane(Pane),
    Tab(Tab),
    Surface(Box<dyn Surface>),
}

pub(crate) struct PreparedLeaf {
    pub(crate) logical_kind: String,
    pub(crate) surface: Box<dyn Surface>,
}

pub(crate) type PreparedLeaves = HashMap<u32, PreparedLeaf>;

type Result<T> = std::result::Result<T, String>;

/// A mismatch halts publication. The caller must not publish any engine in a partially applied batch.
/// Both comparisons exclude content payloads, resource status, and View selections.
pub(crate) fn apply(
    engine: &mut CoreState,
    before: &JournalModel,
    batch: &DomainBatch,
    prepared: &mut PreparedLeaves,
    retired: &mut Vec<Retired>,
) -> Result<()> {
    preflight(engine, before, batch, prepared)?;
    for recorded in &batch.events {
        structure::apply_event(engine, &recorded.event, prepared, retired)?;
    }
    let mut after = before.clone();
    tasty_domain::evolve(&mut after, batch).map_err(|error| error.to_string())?;
    if live::core_canonical(engine) != Canonical::of_journal(&after, &SkipData) {
        return Err("committed live projection differs from the canonical result".into());
    }
    Ok(())
}

fn preflight(
    engine: &CoreState,
    before: &JournalModel,
    batch: &DomainBatch,
    prepared: &PreparedLeaves,
) -> Result<()> {
    if live::core_canonical(engine) != Canonical::of_journal(before, &SkipData) {
        return Err("live projection is not at the command's committed predecessor".into());
    }
    let mut after = before.clone();
    tasty_domain::evolve(&mut after, batch).map_err(|error| error.to_string())?;
    for event in batch.events.iter().map(|recorded| &recorded.event) {
        let required = match event {
            DomainEvent::TabCreated { surface, .. } | DomainEvent::SurfaceSplit { surface, .. } => {
                Some((surface.id, surface.kind.as_str()))
            }
            DomainEvent::SurfaceConverted { id, kind, .. } => Some((*id, kind.as_str())),
            DomainEvent::MetadataSet { .. } | DomainEvent::MetadataRemoved { .. } => {
                return Err("service metadata is not part of the live structure projection".into());
            }
            _ => None,
        };
        if let Some((id, kind)) = required {
            let leaf = prepared
                .get(&id)
                .ok_or_else(|| format!("surface {id} has no prepared kind instance"))?;
            if leaf.surface.surface_id() != Some(id) || leaf.logical_kind != kind {
                return Err(format!(
                    "surface {id} preparation belongs to another identity or kind"
                ));
            }
        }
    }
    Ok(())
}

fn take_prepared(prepared: &mut PreparedLeaves, id: u32) -> Result<Box<dyn Surface>> {
    prepared
        .remove(&id)
        .map(|leaf| leaf.surface)
        .ok_or_else(|| format!("prepared surface {id} was already consumed"))
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
