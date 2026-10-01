//! Persisted launch parameters. Terminal output bytes and parser content never enter this input.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PreparationInput {
    #[serde(default)]
    pub(crate) adopt:Option<AdoptRecipe>,
    #[serde(default)]
    pub(crate) child:Option<ChildRecipe>,
    pub(crate) kind: String,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) params: serde_json::Value,
    pub(crate) shell: Option<ShellRecipe>,
    pub(crate) restore: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShellRecipe {
    pub(crate) executable: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) environment: Vec<(String, String)>,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) scrollback_lines: usize,
    pub(crate) disk_scrollback: bool,
    pub(crate) startup_command: String,
    pub(crate) restore_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectLease {
    pub(crate) effect_id: String,
    pub(crate) operation: tasty_core::OperationId,
    pub(crate) stream: String,
    pub(crate) runtime_epoch: u64,
    pub(crate) resource_generation: u64,
    pub(crate) attempt: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimedPreparation {
    pub(crate) lease: EffectLease,
    pub(crate) input: PreparationInput,
    pub(crate) plan: tasty_core::CreationPlan,
    /// Immutable capture bytes read by the storage worker, never replayed as PTY input.
    pub(crate) capture: Option<Vec<u8>>,
    pub(crate) engine_incarnation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RecordedEffect {
    pub(super) stream: String,
    pub(super) instruction: tasty_core::StructuralEffect,
}

#[derive(Debug,Clone)]
pub(crate) struct ClaimedRetirement {
    pub(crate) lease:EffectLease,
    pub(crate) plan:tasty_core::RetirementPlan,
    pub(crate) engine_incarnation:u64,
}

/// Process-local transfer proof. Recovery never turns an earlier epoch's PTY number into a launch.
#[derive(Debug,Clone,Serialize,Deserialize)]
pub(crate) struct AdoptRecipe {pub pty_id:u32,pub resource_generation:u64,pub runtime_epoch:u64}

/// Registry relation metadata is a service obligation, not a duplicate domain tree.
#[derive(Debug,Clone,Serialize,Deserialize)]
pub(crate) struct ChildRecipe {
    pub parent:u32,pub index:u32,pub workspace:u32,pub runtime_epoch:u64,
    pub cwd:Option<String>,pub role:Option<String>,pub nickname:Option<String>,
    /// The bytes live only in the original bounded request continuation.
    pub has_command:bool,
    #[serde(default)]
    pub replacing:bool,
}
