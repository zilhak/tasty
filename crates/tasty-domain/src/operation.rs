//! Logical preparation and activation facts. No worker handles or OS resource identities live here.

use serde::{Deserialize, Serialize};

use crate::{DataRef, IdKind};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub String);

/// An entity identity fixed when the command is accepted, independent of current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityId {
    pub kind: IdKind,
    pub id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationOutcome {
    Succeeded,
    Failed {
        reason: String,
    },
    Cancelled {
        reason: String,
    },
    Superseded {
        reason: String,
    },
    /// A reconciliation result is required; this is not a successful or cancelled execution.
    Uncertain {
        reason: String,
    },
}

/// Prepared work is replayable without consulting the effect-attempt table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub command_id: String,
    #[serde(default)]
    pub engine_incarnation: u64,
    #[serde(default)]
    pub creation: Option<crate::CreationPlan>,
    #[serde(default)]
    pub assembly:Option<crate::CreationAssembly>,
    #[serde(default)]
    pub retirement:Option<crate::RetirementPlan>,
    pub targets: Vec<EntityId>,
    pub reserved: Vec<EntityId>,
    /// Immutable, non-secret resolved preparation input owned by this journal.
    pub input: DataRef,
    pub activation_generation: u64,
    pub outcome: Option<OperationOutcome>,
    #[serde(default)]
    pub pending_outcome: Option<OperationOutcome>,
    #[serde(default)]
    pub cleanup: Option<crate::CleanupPlan>,
    #[serde(default)]
    pub prepared_data: Option<DataRef>,
    #[serde(default)]
    pub prepared_deferred:bool,
    #[serde(default)]
    pub resource_prepared:bool,
    pub reconciliation_evidence: Option<DataRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationPhase {
    Requested,
    Deferred,
    Ready,
    Exited,
    Failed,
    Retired,
    Uncertain,
}

/// Historical surface activation. Ready alone does not prove a process is alive in this runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    pub generation: u64,
    pub phase: ActivationPhase,
}
