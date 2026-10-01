//! Persistent stream selection is explicit. Runtime IDs, focus and file age never choose a journal.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum EngineSelection {
    Slot {
        slot: u32,
        resume: bool,
    },
    FreshHeadless,
    /// Explicit internal transfer; ordinary startup never selects another journal.
    ImportedSlot {
        source: crate::runtime::journal_payload::import::SourceImport,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EngineBinding {
    pub(crate) journal_id: String,
    pub(crate) stream: String,
    pub(crate) incarnation: u64,
    pub(crate) runtime_epoch: u64,
    pub(crate) published_cut: Option<u64>,
    pub(crate) revision: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundEngine {
    pub(crate) binding: EngineBinding,
    /// One-use bootstrap source; consumed when the initial live projection is built.
    pub(crate) model: tasty_core::JournalModel,
    pub(crate) imported_view: Option<crate::core::layout_persistence::import::ImportedView>,
}
