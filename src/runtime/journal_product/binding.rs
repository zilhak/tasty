//! Persistent stream selection is explicit. Runtime IDs, focus and file age never choose a journal.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum EngineSelection {
    Slot { slot: u32, resume: bool },
    FreshHeadless,
}

#[derive(Debug, Clone)]
pub(crate) struct EngineBinding {
    pub(crate) journal_id: String,
    pub(crate) stream: String,
    pub(crate) incarnation: u64,
    pub(crate) runtime_epoch: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundEngine {
    pub(crate) binding: EngineBinding,
    /// One-use bootstrap source; consumed when the initial live projection is built.
    pub(crate) model: tasty_domain::JournalModel,
}
