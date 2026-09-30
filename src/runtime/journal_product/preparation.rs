//! Persisted launch parameters. Terminal output bytes and parser content never enter this input.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PreparationInput {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EffectLease {
    pub(crate) effect_id: String,
    pub(crate) operation: tasty_domain::OperationId,
    pub(crate) stream: String,
    pub(crate) runtime_epoch: u64,
    pub(crate) resource_generation: u64,
    pub(crate) attempt: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimedPreparation {
    pub(crate) lease: EffectLease,
    pub(crate) input: PreparationInput,
    pub(crate) plan: tasty_domain::CreationPlan,
    pub(crate) engine_incarnation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RecordedEffect {
    pub(super) stream: String,
    pub(super) instruction: tasty_domain::StructuralEffect,
}
