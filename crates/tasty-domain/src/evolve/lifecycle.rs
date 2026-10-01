use super::*;
use crate::{
    Activation, ActivationPhase, DataRef, EntityId, Operation, OperationId, OperationOutcome, Ratio,
};

pub(super) fn prepare(m: &mut JournalModel, operation: Operation) -> Result<()> {
    if operation.id.0.is_empty()
        || operation.command_id.is_empty()
        || (operation.activation_generation == 0 && operation.retirement.is_none() && operation.assembly.is_none())
        || operation.input.0 == 0
        || (operation.creation.is_some() && operation.retirement.is_some())
        || operation.outcome.is_some()
        || operation.pending_outcome.is_some()
        || operation.cleanup.is_some()
        || operation.prepared_data.is_some()
        || operation.reconciliation_evidence.is_some()
    {
        return Err(EvolveError::InvalidFact(
            "invalid operation preparation".into(),
        ));
    }
    if let Some(plan)=&operation.retirement {
        let (_,removed,surfaces)=crate::command::retirement::close_facts(m,plan.target).ok_or_else(||EvolveError::InvalidFact("retirement target cannot close".into()))?;
        if removed!=plan.removed || !surfaces.iter().copied().eq(plan.surfaces.iter().map(|surface|surface.id))
            || plan.tab_parents.len()!=removed.iter().filter(|entity|entity.kind==IdKind::Tab).count()
            || plan.tab_parents.iter().any(|(tab,pane)|!removed.iter().any(|entity|entity.kind==IdKind::Tab && entity.id==*tab) || !m.panes.get(pane).is_some_and(|pane|pane.tabs.contains(tab)))
            || plan.surfaces.iter().any(|target|m.surfaces.get(&target.id).is_none_or(|surface|surface.kind!=target.kind || surface.activation.map(|value|value.generation)!=target.activation_generation)) {
            return Err(EvolveError::InvalidFact("retirement plan does not name the exact existing owners".into()));
        }
    }
    if m.operations.contains_key(&operation.id) {
        return Err(EvolveError::Duplicate(format!(
            "operation:{}",
            operation.id.0
        )));
    }
    let mut reserved = std::collections::BTreeSet::new();
    for entity in &operation.reserved {
        if entity.id == 0
            || exists(m, *entity)
            || !reserved.insert((entity.kind, entity.id))
            || m.operations.values().any(|op| op.reserved.contains(entity))
        {
            return Err(EvolveError::Duplicate(format!(
                "reserved {}:{}",
                entity.kind.label(),
                entity.id
            )));
        }
    }
    if let Some(plan) = &operation.creation {
        let last = m
            .activation_high_water
            .get(&plan.surface.id)
            .copied()
            .unwrap_or(0)
            .max(
                m.surfaces
                    .get(&plan.surface.id)
                    .and_then(|surface| surface.activation)
                    .map_or(0, |activation| activation.generation),
            );
        if last.checked_add(1) != Some(operation.activation_generation) {
            return Err(EvolveError::InvalidFact(
                "activation generation was already reserved or skipped".into(),
            ));
        }
        m.activation_high_water
            .insert(plan.surface.id, operation.activation_generation);
    }
    m.operations.insert(operation.id.clone(), operation);
    Ok(())
}

fn exists(m: &JournalModel, entity: EntityId) -> bool {
    match entity.kind {
        IdKind::Category => m.categories.contains_key(&entity.id),
        IdKind::Workspace => m.workspaces.contains_key(&entity.id),
        IdKind::Pane => m.panes.contains_key(&entity.id),
        IdKind::Tab => m.tabs.contains_key(&entity.id),
        IdKind::Surface => m.surfaces.contains_key(&entity.id),
    }
}

pub(super) fn finish(
    m: &mut JournalModel,
    id: OperationId,
    outcome: OperationOutcome,
    evidence: Option<DataRef>,
) -> Result<()> {
    let op = m
        .operations
        .get_mut(&id)
        .ok_or_else(|| EvolveError::Missing(format!("operation:{}", id.0)))?;
    let allowed = match (&op.outcome, evidence) {
        (None, None) => true,
        (Some(OperationOutcome::Uncertain { .. }), Some(data)) => {
            data.0 != 0
                && !matches!(
                    outcome,
                    OperationOutcome::Uncertain { .. } | OperationOutcome::Superseded { .. }
                )
        }
        _ => false,
    };
    if !allowed {
        return Err(EvolveError::InvalidFact(
            "operation outcome cannot be overwritten without reconciliation".into(),
        ));
    }
    if !matches!(outcome, OperationOutcome::Uncertain { .. }) {
        op.pending_outcome = None;
        op.cleanup = None;
        if !matches!(op.creation.as_ref().map(|plan|&plan.destination),Some(crate::CreationDestination::Assembly {..})) {op.prepared_data = None;}
    }
    op.outcome = Some(outcome);
    op.reconciliation_evidence = evidence;
    Ok(())
}

pub(super) fn activation(
    m: &mut JournalModel,
    id: SurfaceId,
    previous: Option<u64>,
    next: Activation,
) -> Result<()> {
    let surface = get_mut(&mut m.surfaces, IdKind::Surface, id)?;
    let current = surface.activation;
    if current.map(|a| a.generation) != previous
        || next.generation == 0
        || current.is_some_and(|a| {
            next.generation < a.generation
                || (next.generation == a.generation && !phase_transition(a.phase, next.phase))
        })
    {
        return Err(EvolveError::InvalidFact(
            "stale or invalid activation transition".into(),
        ));
    }
    surface.activation = Some(next);
    m.activation_high_water
        .entry(id)
        .and_modify(|last| *last = (*last).max(next.generation))
        .or_insert(next.generation);
    Ok(())
}

fn phase_transition(from: ActivationPhase, to: ActivationPhase) -> bool {
    use ActivationPhase::*;
    matches!(
        (from, to),
        (Requested, Deferred | Ready | Failed | Retired | Uncertain)
            | (Deferred, Requested | Ready | Failed | Retired)
            | (Ready, Exited | Retired | Uncertain)
            | (Exited | Failed, Retired)
            | (Uncertain, Ready | Failed | Retired)
    )
}

pub(super) fn ratio<Id>(layout: &mut SplitTree<Id>, path: &[bool], value: Ratio) -> Result<()> {
    if !value.to_f32().is_finite() || !(0.1..=0.9).contains(&value.to_f32()) {
        return Err(EvolveError::InvalidFact(
            "split ratio is outside 0.1..=0.9".into(),
        ));
    }
    let mut node = layout;
    for second in path {
        node = match node {
            SplitTree::Split {
                first,
                second: other,
                ..
            } => {
                if *second {
                    other
                } else {
                    first
                }
            }
            SplitTree::Leaf(_) => return Err(EvolveError::Missing("split path".into())),
        };
    }
    match node {
        SplitTree::Split { ratio, .. } => {
            *ratio = value;
            Ok(())
        }
        SplitTree::Leaf(_) => Err(EvolveError::Missing("split node".into())),
    }
}

pub(super) fn await_cleanup(
    m: &mut JournalModel,
    id: OperationId,
    outcome: OperationOutcome,
    cleanup: crate::CleanupPlan,
    prepared_data: Option<DataRef>,
    deferred:bool,
) -> Result<()> {
    let operation = m
        .operations
        .get_mut(&id)
        .ok_or_else(|| EvolveError::Missing(format!("operation:{}", id.0)))?;
    if operation.outcome.is_some()
        || operation.pending_outcome.is_some()
        || operation.cleanup.is_some()
        || prepared_data.is_some_and(|data| data.0 == 0)
        || matches!(outcome, OperationOutcome::Uncertain { .. })
    {
        return Err(EvolveError::InvalidFact(
            "operation cannot await cleanup from this state".into(),
        ));
    }
    operation.pending_outcome = Some(outcome);
    operation.cleanup = Some(cleanup);
    operation.prepared_data = prepared_data;
    operation.prepared_deferred=deferred;
    Ok(())
}
