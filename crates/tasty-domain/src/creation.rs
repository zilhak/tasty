//! A fixed structural destination for prepared kind resources. It contains no executable service.

use crate::{DataRef, EntityId, IdKind, OperationId, SplitSpec, SurfaceSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CreationDestination {
    Workspace {
        workspace: u32,
        pane: u32,
        tab: u32,
        name: String,
        category: u32,
        subtitle: String,
        description: String,
    },
    Tab {
        pane: u32,
        tab: u32,
        index: usize,
    },
    Pane {
        target: u32,
        pane: u32,
        tab: u32,
        split: SplitSpec,
    },
    Split {
        target: u32,
        split: SplitSpec,
    },
    Convert {
        surface: u32,
        previous_activation: Option<u64>,
        /// None keeps the name; Some(None) clears it, Some(Some(name)) replaces it.
        explicit_name: Option<Option<String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreationPlan {
    pub destination: CreationDestination,
    pub surface: SurfaceSpec,
    pub tab_name: String,
    pub explicit_name: Option<String>,
}

impl CreationPlan {
    pub fn reserved_ids(&self) -> Vec<EntityId> {
        let entity = |kind, id| EntityId { kind, id };
        let mut result = match &self.destination {
            CreationDestination::Workspace {
                workspace,
                pane,
                tab,
                ..
            } => vec![
                entity(IdKind::Workspace, *workspace),
                entity(IdKind::Pane, *pane),
                entity(IdKind::Tab, *tab),
            ],
            CreationDestination::Tab { tab, .. } => vec![entity(IdKind::Tab, *tab)],
            CreationDestination::Pane { pane, tab, .. } => {
                vec![entity(IdKind::Pane, *pane), entity(IdKind::Tab, *tab)]
            }
            CreationDestination::Split { .. } => Vec::new(),
            CreationDestination::Convert { .. } => return Vec::new(),
        };
        result.push(entity(IdKind::Surface, self.surface.id));
        result
    }

    pub fn target_is_live(&self, model: &crate::JournalModel) -> bool {
        match &self.destination {
            CreationDestination::Workspace { category, .. } => {
                model.categories.contains_key(category)
            }
            CreationDestination::Tab { pane, index, .. } => model
                .panes
                .get(pane)
                .is_some_and(|pane| *index <= pane.tabs.len()),
            CreationDestination::Pane { target, .. } => model.panes.contains_key(target),
            CreationDestination::Split { target, .. } => model.surfaces.contains_key(target),
            CreationDestination::Convert {
                surface,
                previous_activation,
                ..
            } => {
                *surface == self.surface.id
                    && model.surfaces.get(surface).is_some_and(|surface| {
                        surface.activation.map(|activation| activation.generation)
                            == *previous_activation
                    })
            }
        }
    }

    pub fn targets(&self) -> Vec<EntityId> {
        let (kind, id) = match &self.destination {
            CreationDestination::Workspace { category, .. } => (IdKind::Category, *category),
            CreationDestination::Tab { pane, .. } => (IdKind::Pane, *pane),
            CreationDestination::Pane { target, .. } => (IdKind::Pane, *target),
            CreationDestination::Split { target, .. } => (IdKind::Surface, *target),
            CreationDestination::Convert { surface, .. } => (IdKind::Surface, *surface),
        };
        vec![EntityId { kind, id }]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreparationResult {
    Ready { data: Option<DataRef> },
    Failed { reason: String },
}

/// Obligations, not callbacks. The execution adapter records them in the outbox before running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StructuralEffect {
    PrepareSurface {
        operation: OperationId,
        input: DataRef,
        surface: u32,
        kind: String,
        activation_generation: u64,
    },
}

/// Continuations remain under the materialization effect's original Running claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupPlan {
    InstallPrepared {
        surface: u32,
        /// Some(None) retires a legacy owner with no durable activation yet.
        previous_activation: Option<Option<u64>>,
    },
    DiscardPrepared {
        surface: u32,
        activation_generation: u64,
    },
}
