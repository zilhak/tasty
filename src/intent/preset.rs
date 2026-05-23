//! Preset 도메인 Intent 핸들러 + 공유 mutation 함수.
//!
//! 본 모듈은 두 가지 표면을 제공한다:
//!
//! 1. **Intent 핸들러** (`handle`): `dispatch_pending_intents` 가 호출.
//!    inner 함수를 호출해 mutation 후 toast/cascade 처리.
//!
//! 2. **공유 mutation 함수** (`apply_inner`, `save_inner`, `delete_inner`,
//!    `rename_inner`, `capture_inner`): IPC 핸들러가 sync 결과를 돌려주기 위해
//!    직접 호출. 외부 caller 와 동일한 코드 경로를 보장한다.
//!
//! 정책 (TODO 01 결정 P1~P5, action-dispatch.md 참조):
//! - **ApplyPreset focus**: origin 으로 자동 분기 (User → focus=true, Agent → false).
//! - **SavePreset naming**: `explicit_name` 우선, 없으면 `base_name` 으로 store.unique_name 자동.
//! - **SavePreset cascade**: User origin 일 때만 save 후 PresetWindow 자동 오픈 + select.
//!   Agent origin 은 cascade 미수행 (focus 독립성 원칙). `state.dialogs.pending_open_preset_window`
//!   + `pending_preset_window_selection` 으로 main loop 에 신호.
//! - **List/Get**: read-only — Intent 큐 안 거치고 IPC handler 가 직접 처리.

use super::{DispatchedIntent, Intent};
use crate::model::Surface;

/// preset Intent 가 운반하는 캡처된 preset payload.
/// 호출자 (우클릭 / IPC) 가 capture 를 수행한 뒤 핸들러에 그대로 넘긴다 — TODO 01
/// 결정 P3 (CapturePreset 별도 Intent 미설치) 반영.
#[derive(Debug, Clone)]
pub enum ClonedPreset {
    Workspace(tasty_presets::WorkspacePreset),
    Tab(tasty_presets::TabPreset),
    Pane(tasty_presets::PanePreset),
}

impl ClonedPreset {
    pub fn kind(&self) -> tasty_presets::PresetKind {
        match self {
            ClonedPreset::Workspace(_) => tasty_presets::PresetKind::Workspace,
            ClonedPreset::Tab(_) => tasty_presets::PresetKind::Tab,
            ClonedPreset::Pane(_) => tasty_presets::PresetKind::Pane,
        }
    }
}

use crate::state::AppState;
use crate::state::preset_apply::{ApplyError, ApplyOptions};
use tasty_presets::{
    CaptureOptions, CapturedSurfaceMeta, PanePreset, PresetError, PresetKind, TabPreset,
    WorkspacePreset,
};

// ───────────────────────────────── Intent dispatcher ─────────────────────────────────

/// preset 도메인 분기 핸들러. `dispatch_pending_intents` 에서 호출.
pub fn handle(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    intent: &DispatchedIntent,
) {
    match &intent.body {
        Intent::ApplyPreset { kind, name } => apply(state, engine, intent, *kind, name),
        Intent::SavePreset {
            base_name,
            explicit_name,
            overwrite,
            preset,
        } => save(
            state,
            engine,
            intent,
            base_name,
            explicit_name.as_deref(),
            *overwrite,
            preset,
        ),
        Intent::DeletePreset { kind, name } => delete(state, engine, *kind, name),
        Intent::RenamePreset { kind, from, to } => rename(state, engine, *kind, from, to),
        _ => {}
    }
}

fn apply(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    intent: &DispatchedIntent,
    kind: PresetKind,
    name: &str,
) {
    // P1: focus 정책은 origin 으로 자동 분기.
    let focus = intent.origin.is_user();
    let options = ApplyOptions { focus };

    if let Err(e) = apply_inner(state, engine, kind, name, None, None, options) {
        tracing::warn!("preset apply failed: {e}");
        state.toasts.push(
            crate::i18n::t("preset.toast.apply_failed"),
            crate::ui::ToastKind::Error,
            crate::ui::ToastScope::Window,
        );
    }
}

fn save(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    intent: &DispatchedIntent,
    base_name: &str,
    explicit_name: Option<&str>,
    overwrite: bool,
    preset: &ClonedPreset,
) {
    let kind = preset.kind();
    let save_result = save_inner(state, engine, base_name, explicit_name, overwrite, preset.clone());

    let toast_key = match (&save_result, kind) {
        (Ok(_), PresetKind::Workspace) => "preset.toast.saved_workspace",
        (Ok(_), PresetKind::Tab) => "preset.toast.saved_tab",
        (Ok(_), PresetKind::Pane) => "preset.toast.saved_pane",
        (Err(_), _) => "preset.toast.save_failed",
    };
    let toast_kind = if save_result.is_ok() {
        crate::ui::ToastKind::Info
    } else {
        crate::ui::ToastKind::Error
    };
    state.toasts.push(
        crate::i18n::t(toast_key),
        toast_kind,
        crate::ui::ToastScope::Window,
    );

    let saved_name = match save_result {
        Ok(SaveOutcome::Saved(n)) => n,
        Ok(SaveOutcome::SkippedExists) => return,
        Err(e) => {
            tracing::warn!("preset save failed: {e}");
            return;
        }
    };

    // User origin cascade: save 후 PresetWindow 자동 오픈 + select.
    // Agent origin 은 cascade 미수행 (focus 독립성).
    if intent.origin.is_user() {
        state.dialogs.pending_open_preset_window = true;
        state.dialogs.pending_preset_window_selection = Some((kind, saved_name));
    }
}

fn delete(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    kind: PresetKind,
    name: &str,
) {
    if let Err(e) = delete_inner(state, engine, kind, name) {
        tracing::warn!("preset delete failed: {e}");
    }
}

fn rename(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    kind: PresetKind,
    from: &str,
    to: &str,
) {
    if let Err(e) = rename_inner(state, engine, kind, from, to) {
        tracing::warn!("preset rename failed: {e}");
    }
}

// ───────────────────────────────── Shared mutation API ─────────────────────────────────

/// `apply_*_preset` 결과를 IPC 가 그대로 응답에 실을 수 있도록 enum 으로 캡슐화.
#[derive(Debug, Clone)]
pub enum ApplyOutcome {
    Workspace { workspace_id: u32 },
    Tab { tab_id: u32 },
    Pane { pane_id: u32 },
}

/// `save_inner` 결과 — 충돌 시 skip 된 경우를 명시.
#[derive(Debug, Clone)]
pub enum SaveOutcome {
    Saved(String),
    /// `overwrite=false` + 이미 존재 → 저장 skip.
    SkippedExists,
}

/// 공유 mutation API: 발생 가능한 실패를 모두 enum 으로 노출.
#[derive(Debug)]
pub enum PresetMutationError {
    StoreUnavailable,
    NotFound { kind: PresetKind, name: String },
    Apply(ApplyError),
    Store(PresetError),
}

impl std::fmt::Display for PresetMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreUnavailable => write!(f, "preset_store unavailable"),
            Self::NotFound { kind, name } => {
                write!(f, "preset not found: {}/{name}", kind.as_str())
            }
            Self::Apply(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PresetMutationError {}

/// preset_store 잠금 + clone. lock 안 ↔ apply 본체를 분리해 critical section 을 짧게 유지.
fn clone_preset_from_store(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    kind: PresetKind,
    name: &str,
) -> Result<Option<ClonedPreset>, PresetMutationError> {
    let arc = engine
        .preset_store
        .as_ref()
        .ok_or(PresetMutationError::StoreUnavailable)?;
    let guard = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
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

/// Preset 적용. store 에서 clone 후 lock 해제하고 본체를 호출한다.
/// `target_pane_id` / `target_workspace_id` 는 tab/pane apply 시에만 의미가 있다.
pub fn apply_inner(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    kind: PresetKind,
    name: &str,
    target_pane_id: Option<u32>,
    target_workspace_id: Option<u32>,
    options: ApplyOptions,
) -> Result<ApplyOutcome, PresetMutationError> {
    let cloned = clone_preset_from_store(state, engine, kind, name)?.ok_or_else(|| {
        PresetMutationError::NotFound {
            kind,
            name: name.to_string(),
        }
    })?;

    match cloned {
        ClonedPreset::Workspace(p) => {
            let idx = state
                .apply_workspace_preset(engine, &p, options)
                .map_err(PresetMutationError::Apply)?;
            let workspace_id = engine.workspaces[idx].id;
            Ok(ApplyOutcome::Workspace { workspace_id })
        }
        ClonedPreset::Tab(p) => {
            let tab_id = state
                .apply_tab_preset(engine, &p, target_pane_id, options)
                .map_err(PresetMutationError::Apply)?;
            Ok(ApplyOutcome::Tab { tab_id })
        }
        ClonedPreset::Pane(p) => {
            let pane_id = state
                .apply_pane_preset(engine, &p, target_workspace_id, options)
                .map_err(PresetMutationError::Apply)?;
            Ok(ApplyOutcome::Pane { pane_id })
        }
    }
}

/// Preset 저장. `explicit_name` 이 있으면 사용 (overwrite=false 면 충돌 시 skip),
/// 없으면 `base_name` 기반 unique_name 자동 부여.
pub fn save_inner(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    base_name: &str,
    explicit_name: Option<&str>,
    overwrite: bool,
    preset: ClonedPreset,
) -> Result<SaveOutcome, PresetMutationError> {
    let arc = engine
        .preset_store
        .as_ref()
        .ok_or(PresetMutationError::StoreUnavailable)?;
    let mut store = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };

    let kind = preset.kind();
    let name = match explicit_name {
        Some(n) => {
            if !overwrite {
                let exists = match kind {
                    PresetKind::Workspace => store.get_workspace(n).is_some(),
                    PresetKind::Tab => store.get_tab(n).is_some(),
                    PresetKind::Pane => store.get_pane(n).is_some(),
                };
                if exists {
                    tracing::warn!("SavePreset: name '{n}' exists, overwrite=false → skip");
                    return Ok(SaveOutcome::SkippedExists);
                }
            }
            n.to_string()
        }
        None => store.unique_name(kind, base_name),
    };

    let result: Result<(), PresetError> = match preset {
        ClonedPreset::Workspace(mut p) => {
            p.name = name.clone();
            if overwrite {
                store.save_workspace_overwrite(p)
            } else {
                store.save_workspace(p)
            }
        }
        ClonedPreset::Tab(mut p) => {
            p.name = name.clone();
            if overwrite {
                store.save_tab_overwrite(p)
            } else {
                store.save_tab(p)
            }
        }
        ClonedPreset::Pane(mut p) => {
            p.name = name.clone();
            if overwrite {
                store.save_pane_overwrite(p)
            } else {
                store.save_pane(p)
            }
        }
    };

    result
        .map(|_| SaveOutcome::Saved(name))
        .map_err(PresetMutationError::Store)
}

pub fn delete_inner(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    kind: PresetKind,
    name: &str,
) -> Result<(), PresetMutationError> {
    let arc = engine
        .preset_store
        .as_ref()
        .ok_or(PresetMutationError::StoreUnavailable)?;
    let mut store = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
    store.delete(kind, name).map_err(PresetMutationError::Store)
}

pub fn rename_inner(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    kind: PresetKind,
    from: &str,
    to: &str,
) -> Result<(), PresetMutationError> {
    let arc = engine
        .preset_store
        .as_ref()
        .ok_or(PresetMutationError::StoreUnavailable)?;
    let mut store = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
    store
        .rename(kind, from, to)
        .map_err(PresetMutationError::Store)
}

/// Surface 식별자로 워크스페이스/탭/페인을 캡처해 ClonedPreset 으로 변환.
/// IPC `preset.capture` 가 사용. UI 우클릭 경로는 자신이 직접 capture 한 뒤
/// `Intent::SavePreset { preset: ClonedPreset, .. }` 로 발화하므로 별도 경로.
pub fn capture_inner(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    kind: PresetKind,
    source_id: u32,
) -> Result<(ClonedPreset, String), String> {
    let registry = engine.surface_registry.clone();
    let mut capture = move |s: &dyn Surface| -> Option<CapturedSurfaceMeta> {
        let def = registry.get(s.kind())?;
        let params = (def.snapshot)(s)?;
        Some(CapturedSurfaceMeta {
            kind: s.kind().to_string(),
            params,
        })
    };

    match kind {
        PresetKind::Workspace => {
            let ws = engine
                .workspaces
                .iter()
                .find(|w| w.id == source_id)
                .ok_or_else(|| format!("Workspace id {source_id} not found"))?;
            let base = if ws.name.is_empty() {
                "workspace".to_string()
            } else {
                ws.name.clone()
            };
            let preset =
                WorkspacePreset::from_workspace(ws, &mut capture, CaptureOptions::default())
                    .ok_or_else(|| "workspace capture failed".to_string())?;
            Ok((ClonedPreset::Workspace(preset), base))
        }
        PresetKind::Tab => {
            let pane_id = engine
                .find_pane_for_tab(source_id)
                .ok_or_else(|| format!("Tab id {source_id} not found"))?;
            for ws in &engine.workspaces {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if tab.id == source_id {
                            let base = tab
                                .explicit_name
                                .clone()
                                .unwrap_or_else(|| tab.name.clone());
                            let base = if base.is_empty() {
                                "tab".to_string()
                            } else {
                                base
                            };
                            let preset =
                                TabPreset::from_tab(tab, &mut capture, CaptureOptions::default())
                                    .ok_or_else(|| "tab capture failed".to_string())?;
                            return Ok((ClonedPreset::Tab(preset), base));
                        }
                    }
                }
            }
            Err(format!("Tab id {source_id} not found"))
        }
        PresetKind::Pane => {
            for ws in &engine.workspaces {
                if let Some(pane) = ws.pane_layout().find_pane(source_id) {
                    let preset =
                        PanePreset::from_pane(pane, &mut capture, CaptureOptions::default())
                            .ok_or_else(|| "pane capture failed".to_string())?;
                    return Ok((ClonedPreset::Pane(preset), "pane".to_string()));
                }
            }
            Err(format!("Pane id {source_id} not found"))
        }
    }
}
