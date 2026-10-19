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
    /// Successful legacy import in the original resumed incarnation; App owns memory cleanup.
    #[cfg(feature = "gui")]
    pub(crate) restore_legacy_metadata: bool,
    pub(crate) imported_view: Option<crate::core::layout_persistence::import::ImportedView>,
    /// 엔진은 열었지만 일부 복원 자료를 쓰지 못한 사실. App이 알림으로 옮긴다.
    pub(crate) issues: Vec<BindingIssue>,
}

/// 엔진을 열며 잃은 복원 자료. 원본은 그대로 두거나 `preserved`에 보존했다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingIssue {
    /// 저장된 View 선택을 읽지 못해 기본 선택을 쓴다.
    ViewUnreadable {
        slot: u32,
        preserved: String,
        cause: String,
    },
    /// 이전 형식 슬롯 파일을 레이아웃으로 해석하지 못해 초기 상태로 시작했다.
    LegacyUnparsable { path: String, cause: String },
    /// 이전 형식 슬롯 파일을 읽지 못해 초기 상태로 시작했다.
    LegacyUnreadable {
        path: String,
        permission_denied: bool,
        cause: String,
    },
    /// 가져왔지만 터미널 일부의 scrollback 파일이 없거나 읽히지 않았다.
    MissingScrollback { count: usize, path: String },
}
