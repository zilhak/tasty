use super::*;
use crate::{CloseTarget, IdKind, Operation, RetiredSurface, RetirementPlan, StructuralEffect};
pub(super) fn decide(
    model: &JournalModel,
    command: &StructuralCommand,
) -> Result<StructuralDecision, Rejection> {
    let StructuralCommand::Replace {
        operation,
        command_id,
        input,
        replacement,
        expected,
    } = command
    else {
        unreachable!()
    };
    if replacement.source == replacement.target {
        return Ok(StructuralDecision {
            events: Vec::new(),
            effects: Vec::new(),
            result: StructuralResult::Moved { moved: false },
            completed_command: None,
        });
    }
    let Some(source) = replacement.surfaces(model, replacement.source) else {
        return Ok(noop());
    };
    let Some(target) = replacement.surfaces(model, replacement.target) else {
        return Ok(noop());
    };
    let observed: Vec<_> = source
        .into_iter()
        .chain(target)
        .map(|id| {
            let surface = &model.surfaces[&id];
            RetiredSurface {
                id,
                kind: surface.kind.clone(),
                activation_generation: surface.activation.map(|value| value.generation),
            }
        })
        .collect();
    if &observed != expected {
        return Err(Rejection(
            "replacement source or target instance changed before commit".into(),
        ));
    }
    let (_, removed) = replacement.simulate(model)?;
    let surfaces = removed
        .iter()
        .filter(|entity| entity.kind == IdKind::Surface)
        .map(|entity| {
            let surface = &model.surfaces[&entity.id];
            RetiredSurface {
                id: entity.id,
                kind: surface.kind.clone(),
                activation_generation: surface.activation.map(|value| value.generation),
            }
        })
        .collect();
    let tab_parents = removed
        .iter()
        .filter(|entity| entity.kind == IdKind::Tab)
        .map(|entity| (entity.id, model.tabs[&entity.id].pane))
        .collect();
    let target = match replacement.target.kind {
        IdKind::Surface => CloseTarget::Surface(replacement.target.id),
        IdKind::Tab => CloseTarget::Tab(replacement.target.id),
        IdKind::Pane => CloseTarget::Pane(replacement.target.id),
        _ => return Err(Rejection("unsupported replacement target".into())),
    };
    let plan = RetirementPlan {
        replacement: Some(*replacement),
        target,
        removed: removed.clone(),
        surfaces,
        tab_parents,
        is_user_close: false,
        remote_user_close: false,
        undo: None,
    };
    let record = Operation {
        id: operation.clone(),
        command_id: command_id.clone(),
        engine_incarnation: model.engine_incarnation,
        creation: None,
        assembly: None,
        retirement: Some(plan.clone()),
        forward: false,
        targets: vec![replacement.source, replacement.target],
        reserved: Vec::new(),
        input: *input,
        activation_generation: 0,
        outcome: None,
        pending_outcome: None,
        cleanup: None,
        prepared_data: None,
        prepared_deferred: false,
        resource_prepared: false,
        reconciliation_evidence: None,
    };
    Ok(StructuralDecision {
        events: vec![
            DomainEvent::OperationPrepared {
                operation: Box::new(record),
            },
            DomainEvent::StructureReplaced {
                replacement: *replacement,
                removed,
            },
        ],
        effects: vec![StructuralEffect::RetireSurfaces {
            operation: operation.clone(),
            plan,
        }],
        result: StructuralResult::Pending {
            operation: operation.clone(),
        },
        completed_command: None,
    })
}
fn noop() -> StructuralDecision {
    StructuralDecision {
        events: Vec::new(),
        effects: Vec::new(),
        result: StructuralResult::Moved { moved: false },
        completed_command: None,
    }
}
