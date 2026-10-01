//! Structural inputs contain fixed identities and values, never live services or an ID allocator.

mod bootstrap;
mod creation;
mod metadata;

use serde::{Deserialize, Serialize};
use tasty_model::WorkspaceAttachMapping;

use crate::{DomainEvent, JournalModel, Ratio};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StructuralCommand {
    RetireEngine {
        expected_incarnation: u64,
    },
    OpenEngine {
        expected_incarnation: u64,
        reset_structure: bool,
        normal_category_name: String,
    },
    PrepareCreation {
        operation: crate::OperationId,
        command_id: String,
        input: crate::DataRef,
        plan: crate::CreationPlan,
    },
    CancelUnstartedCreation {
        operation: crate::OperationId,
        reason: String,
    },
    RejectInstallation {
        operation: crate::OperationId,
        reason: String,
    },
    FinishCleanup {
        operation: crate::OperationId,
    },
    MarkPreparationUncertain {
        operation:crate::OperationId,
        reason:String,
    },
    FinishCreation {
        operation: crate::OperationId,
        result: crate::PreparationResult,
    },
    ResetCategories,
    CreateCategory {
        reserved_id: u32,
        name: String,
    },
    RenameCategory {
        id: u32,
        name: String,
    },
    DeleteCategory {
        id: u32,
    },
    ReorderCategory {
        id: u32,
        to_index: usize,
    },
    SetWorkspaceCategory {
        workspace_id: u32,
        category: u32,
    },
    UpdateWorkspaceMeta {
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    },
    SetWorkspaceAttachMapping {
        workspace_id: u32,
        mapping: Option<WorkspaceAttachMapping>,
    },
    MoveWorkspace {
        workspace_id: u32,
        to_index: usize,
    },
    RenameTab {
        tab_id: u32,
        name: Option<String>,
    },
    MoveTab {
        pane_id: u32,
        tab_id: u32,
        to_index: usize,
    },
    SetPaneRatio {
        workspace_id: u32,
        path: Vec<bool>,
        expected_leaves: Vec<u32>,
        expected_revision: u64,
        ratio: Ratio,
    },
    SetSurfaceRatio {
        tab_id: u32,
        path: Vec<bool>,
        expected_leaves: Vec<u32>,
        expected_revision: u64,
        ratio: Ratio,
    },
}

impl StructuralCommand {
    /// Fresh identities required by this command. Reservation is an admission task, never decide I/O.
    pub fn reserved_ids(&self) -> Vec<crate::EntityId> {
        match self {
            Self::PrepareCreation { plan, .. } => plan.reserved_ids(),
            Self::CreateCategory { reserved_id, .. } => vec![crate::EntityId {
                kind: crate::IdKind::Category,
                id: *reserved_id,
            }],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructuralResult {
    EngineOpened {
        incarnation: u64,
    },
    Pending {
        operation: crate::OperationId,
    },
    Created {
        workspace: Option<u32>,
        pane: Option<u32>,
        tab: Option<u32>,
        surface: u32,
    },
    Failed {
        reason: String,
    },
    Updated,
    CreatedCategory {
        id: u32,
    },
    Moved {
        moved: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuralDecision {
    pub events: Vec<DomainEvent>,
    pub result: StructuralResult,
    pub effects: Vec<crate::StructuralEffect>,
    pub completed_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Rejection(pub String);

/// Decide on immutable structure and validate the complete candidate without publishing it.
pub fn decide_structure(
    model: &JournalModel,
    command: &StructuralCommand,
) -> Result<StructuralDecision, Rejection> {
    if model.engine_retired
        && !matches!(
            command,
            StructuralCommand::OpenEngine { .. }
                | StructuralCommand::RetireEngine { .. }
                | StructuralCommand::FinishCreation { .. }
                | StructuralCommand::FinishCleanup { .. }
                | StructuralCommand::RejectInstallation { .. }
                | StructuralCommand::MarkPreparationUncertain { .. }
                | StructuralCommand::CancelUnstartedCreation { .. }
        )
    {
        return Err(Rejection("engine binding has been retired".into()));
    }
    let decision = match command {
        StructuralCommand::OpenEngine { .. } | StructuralCommand::RetireEngine { .. } => {
            bootstrap::decide(model, command)?
        }
        StructuralCommand::PrepareCreation { .. }
        | StructuralCommand::FinishCreation { .. }
        | StructuralCommand::FinishCleanup { .. }
        | StructuralCommand::CancelUnstartedCreation { .. }
        | StructuralCommand::RejectInstallation { .. }
                | StructuralCommand::MarkPreparationUncertain { .. } => creation::decide(model, command)?,
        _ => metadata::decide(model, command)?,
    };
    let mut candidate = model.clone();
    let after = model.applied.revision.unwrap_or(0);
    let batch_id = model
        .applied
        .batch
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Rejection("journal batch range exhausted".into()))?;
    let events = decision
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let revision = after
                .checked_add(index as u64 + 1)
                .ok_or_else(|| Rejection("structure revision range exhausted".into()))?;
            Ok(crate::RecordedEvent {
                revision,
                event: event.clone(),
            })
        })
        .collect::<Result<Vec<_>, Rejection>>()?;
    crate::evolve(&mut candidate, &crate::DomainBatch { batch_id, events })
        .map_err(|e| Rejection(e.to_string()))?;
    Ok(decision)
}
