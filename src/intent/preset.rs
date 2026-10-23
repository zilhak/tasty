//! 프리셋 저장과 읽기 값 처리. 구조 적용은 App journal assembly가 소유한다.
//! 저장 완료의 창 표시 요청은 origin에 따르며 자원 생성은 이 모듈에서 실행하지 않는다.

use super::{DispatchedIntent, Intent};
use crate::runtime::engine_access::{EngineMut, EngineRef};

pub use crate::app::preset_commands::Ticket as MutationTicket;
pub use tasty_presets::OwnedPreset as ClonedPreset;

use crate::state::RequestContext;
use tasty_presets::{PresetError, PresetKind};

pub fn handle(
    _core: &crate::app::services::AppServices,
    _state: &mut RequestContext,
    _engine: &mut EngineMut<'_>,
    intent: &DispatchedIntent,
) {
    if matches!(
        intent.body,
        Intent::ApplyPreset { .. } | Intent::CapturePreset { .. }
    ) {
        tracing::error!("preset operation bypassed the journal command boundary");
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SaveOutcome {
    Saved(String),
    /// 이름이 이미 있고 overwrite=false여서 저장하지 않았다.
    SkippedExists,
}

#[derive(Debug)]
pub enum PresetMutationError {
    Store(PresetError),
    #[cfg(feature = "gui")]
    Conflict(String),
    Transport {
        message: String,
        unknown: bool,
    },
    Cache {
        message: String,
        saved: bool,
        partial: bool,
        completed_edits: usize,
    },
}

impl std::fmt::Display for PresetMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => write!(f, "{e}"),
            #[cfg(feature = "gui")]
            Self::Conflict(message) => f.write_str(message),
            Self::Transport { message, .. } | Self::Cache { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for PresetMutationError {}

// 적용 중 저장소 잠금을 유지하지 않도록 프리셋을 복사한다.
pub(crate) fn clone_preset_from_store(
    core: &crate::app::services::AppServices,
    kind: PresetKind,
    name: &str,
) -> Result<Option<ClonedPreset>, PresetMutationError> {
    let guard = crate::poison::recover_mutex(
        core.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    );
    let cloned = match kind {
        PresetKind::Workspace => guard
            .get_workspace(name)
            .cloned()
            .map(ClonedPreset::Workspace),
        PresetKind::Tab => guard.get_tab(name).cloned().map(ClonedPreset::Tab),
        PresetKind::Pane => guard.get_pane(name).cloned().map(ClonedPreset::Pane),
    };
    Ok(cloned)
}

/// The cache is updated before this ticket yields a committed save result.
pub fn save_inner(
    core: &crate::app::services::AppServices,
    base_name: &str,
    explicit_name: Option<&str>,
    overwrite: bool,
    preset: ClonedPreset,
) -> MutationTicket<SaveOutcome> {
    core.preset_files.save(
        base_name.into(),
        explicit_name.map(str::to_owned),
        overwrite,
        preset,
    )
}

pub fn delete_inner(
    core: &crate::app::services::AppServices,
    kind: PresetKind,
    name: &str,
) -> MutationTicket<()> {
    core.preset_files.delete(kind, name.into())
}

pub fn rename_inner(
    core: &crate::app::services::AppServices,
    kind: PresetKind,
    from: &str,
    to: &str,
) -> MutationTicket<()> {
    core.preset_files.rename(kind, from.into(), to.into())
}

/// kind에 맞는 워크스페이스·탭·pane ID로 프리셋을 캡처한다.
/// Live-only 호환 조회이며 지연 자료가 있으면 오류를 반환한다. 제품 캡처는 journal worker에 합류한다.
pub fn capture_inner(
    presentation: &dyn crate::model::StructurePresentation,
    engine: &EngineRef<'_>,
    kind: PresetKind,
    source_id: u32,
) -> Result<(ClonedPreset, String), String> {
    crate::intent::preset_capture::capture_draft(presentation, engine, kind, source_id)?
        .finish_live()
}
