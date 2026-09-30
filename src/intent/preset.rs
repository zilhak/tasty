//! 프리셋 Intent 처리와 IPC가 함께 사용하는 적용·저장 함수.
//! Intent 경로는 origin에 따라 포커스와 창 열기를 처리하며, IPC는 inner 함수를 직접 호출해 응답한다.

use super::{DispatchedIntent, Intent};
use crate::core::engine_access::{EngineMut, EngineRef};

/// 호출자가 미리 캡처해 큐에 넣는 프리셋 데이터.
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

use crate::intent::preset_capture::{
    capture_pane_preset, capture_tab_preset, capture_workspace_preset,
};
use crate::state::RequestContext;
use crate::state::preset_apply::{ApplyError, ApplyOptions};
use tasty_presets::{PresetError, PresetKind};

pub fn handle(
    core: &crate::core::Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    intent: &DispatchedIntent,
) {
    match &intent.body {
        Intent::ApplyPreset {
            kind,
            name,
            category,
        } => apply(
            core,
            state,
            engine,
            intent,
            PresetApplyTarget {
                kind: *kind,
                name,
                target_pane_id: None,
                target_workspace_id: None,
                category: *category,
            },
        ),
        Intent::SavePreset {
            base_name,
            explicit_name,
            overwrite,
            preset,
        } => save(
            core,
            state,
            engine,
            intent,
            PresetSaveRequest {
                base_name,
                explicit_name: explicit_name.as_deref(),
                overwrite: *overwrite,
                preset,
            },
        ),
        _ => {}
    }
}

fn apply(
    core: &crate::core::Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    intent: &DispatchedIntent,
    target: PresetApplyTarget,
) {
    let focus = intent.origin.is_user();
    let options = ApplyOptions { focus };

    if let Err(e) = apply_inner(core, state, engine, target, options) {
        tracing::warn!("preset apply failed: {e}");
        #[cfg(feature = "gui")]
        if intent.origin.is_user() {
            // mirror 거절은 다른 mirror 구조 변경 차단과 같은 안내를 쓴다.
            let (key, kind) = if e.is_mirror_refusal() {
                (
                    "attach.toast.mirror_structural_blocked",
                    crate::model::toast_kind::ToastKind::Warning,
                )
            } else {
                (
                    "preset.toast.apply_failed",
                    crate::model::toast_kind::ToastKind::Error,
                )
            };
            state.toasts.push(
                crate::i18n::t(key),
                kind,
                crate::model::toast_kind::ToastScope::Window,
            );
        }
    }
}

fn save(
    core: &crate::core::Core,
    state: &mut RequestContext,
    _engine: &mut crate::core::CoreState,
    intent: &DispatchedIntent,
    request: PresetSaveRequest,
) {
    let kind = request.preset.kind();
    let save_result = save_inner(
        core,
        request.base_name,
        request.explicit_name,
        request.overwrite,
        request.preset.clone(),
    );

    let toast_key = match (&save_result, kind) {
        (Ok(_), PresetKind::Workspace) => "preset.toast.saved_workspace",
        (Ok(_), PresetKind::Tab) => "preset.toast.saved_tab",
        (Ok(_), PresetKind::Pane) => "preset.toast.saved_pane",
        (Err(_), _) => "preset.toast.save_failed",
    };
    // 실패 토스트는 사용자 요청에만 표시한다. 성공 토스트는 origin과 관계없이 표시한다.
    let show_toast = save_result.is_ok() || intent.origin.is_user();
    #[cfg(feature = "gui")]
    if show_toast {
        let toast_kind = if save_result.is_ok() {
            crate::model::toast_kind::ToastKind::Info
        } else {
            crate::model::toast_kind::ToastKind::Error
        };
        state.toasts.push(
            crate::i18n::t(toast_key),
            toast_kind,
            crate::model::toast_kind::ToastScope::Window,
        );
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = (toast_key, show_toast); // reason: 헤드리스에는 토스트 표시가 없다.
    }

    let saved_name = match save_result {
        Ok(SaveOutcome::Saved(n)) => n,
        Ok(SaveOutcome::SkippedExists) => return,
        Err(e) => {
            tracing::warn!("preset save failed: {e}");
            return;
        }
    };

    // 저장한 프리셋 창을 여는 요청은 GUI의 사용자 동작에서만 만든다.
    #[cfg(feature = "gui")]
    if intent.origin.is_user() {
        state.dialogs.pending_open_preset_window = true;
        state.dialogs.pending_preset_window_selection = Some((kind, saved_name));
    }
    // reason: 헤드리스는 토스트와 프리셋 창 요청을 만들지 않아 이 값들을 사용하지 않는다.
    #[cfg(not(feature = "gui"))]
    let _ = (state, intent, kind, saved_name);
}

/// 적용으로 생성한 대상의 ID. IPC 응답에서도 사용한다.
#[derive(Debug, Clone)]
pub enum ApplyOutcome {
    Workspace { workspace_id: u32 },
    Tab { tab_id: u32 },
    Pane { pane_id: u32 },
}

#[derive(Debug, Clone)]
pub enum SaveOutcome {
    Saved(String),
    /// 이름이 이미 있고 overwrite=false여서 저장하지 않았다.
    SkippedExists,
}

#[derive(Debug)]
pub enum PresetMutationError {
    NotFound { kind: PresetKind, name: String },
    Apply(ApplyError),
    Store(PresetError),
}

impl std::fmt::Display for PresetMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { kind, name } => {
                write!(f, "preset not found: {}/{name}", kind.as_str())
            }
            Self::Apply(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PresetMutationError {}

impl PresetMutationError {
    /// 대상이 mirror workspace라 적용하지 않았다는 거절인지 확인한다.
    #[cfg(any(feature = "gui", test))]
    fn is_mirror_refusal(&self) -> bool {
        matches!(
            self,
            Self::Apply(ApplyError::Other(e))
                if e.downcast_ref::<crate::core::MirrorStructuralBlocked>().is_some()
        )
    }
}

/// tab·pane preset이 들어갈 workspace가 mirror면 그 index를 반환한다.
/// 대상을 생략하면 적용 코드처럼 활성 workspace를 본다. 대상이 없으면 적용 코드가 오류를 낸다.
/// workspace preset은 새 workspace를 만들므로 검사하지 않는다.
fn mirror_target_index(
    state: &RequestContext,
    engine: &crate::core::CoreState,
    target: &PresetApplyTarget,
) -> Option<usize> {
    let active = || {
        engine
            .workspaces()
            .len()
            .checked_sub(1)
            .map(|last| state.active_workspace_index(engine).min(last))
    };
    let idx = match target.kind {
        PresetKind::Tab => match target.target_pane_id {
            Some(pid) => engine.find_workspace_index_for_pane(pid)?,
            None => active()?,
        },
        PresetKind::Pane => match target.target_workspace_id {
            Some(ws_id) => engine.find_workspace_index_for_id(ws_id)?,
            None => active()?,
        },
        PresetKind::Workspace => return None,
    };
    engine.workspace_at(idx).filter(|ws| ws.mirror).map(|_| idx)
}

// 적용 중 저장소 잠금을 유지하지 않도록 프리셋을 복사한다.
fn clone_preset_from_store(
    core: &crate::core::Core,
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

/// 적용할 프리셋과 대상. Tab은 target_pane_id, Pane은 target_workspace_id,
/// Workspace는 category를 사용하며 다른 종류의 필드는 무시한다.
pub struct PresetApplyTarget<'a> {
    pub kind: PresetKind,
    pub name: &'a str,
    pub target_pane_id: Option<u32>,
    pub target_workspace_id: Option<u32>,
    pub category: Option<crate::model::WorkspaceCategoryId>,
}

pub struct PresetSaveRequest<'a> {
    pub base_name: &'a str,
    pub explicit_name: Option<&'a str>,
    pub overwrite: bool,
    pub preset: &'a ClonedPreset,
}

pub fn apply_inner(
    core: &crate::core::Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    target: PresetApplyTarget,
    options: ApplyOptions,
) -> Result<ApplyOutcome, PresetMutationError> {
    let cloned = clone_preset_from_store(core, target.kind, target.name)?.ok_or_else(|| {
        PresetMutationError::NotFound {
            kind: target.kind,
            name: target.name.to_string(),
        }
    })?;

    // mirror 트리는 원격이 소유한다. preset은 원격으로 전달할 수 없으므로 만들기 전에 거절한다.
    if let Some(workspace_index) = mirror_target_index(state, engine, &target) {
        return Err(PresetMutationError::Apply(ApplyError::Other(
            anyhow::Error::new(crate::core::MirrorStructuralBlocked {
                workspace_index,
                forwarded: false,
            }),
        )));
    }

    match cloned {
        ClonedPreset::Workspace(p) => {
            let idx = state
                .apply_workspace_preset(engine, &p, target.category, options)
                .map_err(PresetMutationError::Apply)?;
            let workspace_id = engine
                .workspace_at(idx)
                .expect("workspace index is valid")
                .id;
            Ok(ApplyOutcome::Workspace { workspace_id })
        }
        ClonedPreset::Tab(p) => {
            let tab_id = state
                .apply_tab_preset(engine, &p, target.target_pane_id, options)
                .map_err(PresetMutationError::Apply)?;
            Ok(ApplyOutcome::Tab { tab_id })
        }
        ClonedPreset::Pane(p) => {
            let pane_id = state
                .apply_pane_preset(engine, &p, target.target_workspace_id, options)
                .map_err(PresetMutationError::Apply)?;
            Ok(ApplyOutcome::Pane { pane_id })
        }
    }
}

/// 이름을 직접 지정하고 overwrite=false이면 충돌 시 None을 반환한다.
/// 이름을 지정하지 않으면 base_name으로 중복되지 않는 이름을 만든다.
fn resolve_save_name(
    store: &tasty_presets::PresetStore,
    kind: PresetKind,
    base_name: &str,
    explicit_name: Option<&str>,
    overwrite: bool,
) -> Option<String> {
    match explicit_name {
        Some(n) => {
            if !overwrite {
                let exists = match kind {
                    PresetKind::Workspace => store.get_workspace(n).is_some(),
                    PresetKind::Tab => store.get_tab(n).is_some(),
                    PresetKind::Pane => store.get_pane(n).is_some(),
                };
                if exists {
                    tracing::warn!("SavePreset: name '{n}' exists, overwrite=false → skip");
                    return None;
                }
            }
            Some(n.to_string())
        }
        None => Some(store.unique_name(kind, base_name)),
    }
}

fn store_preset(
    store: &mut tasty_presets::PresetStore,
    preset: ClonedPreset,
    name: String,
    overwrite: bool,
) -> Result<(), PresetError> {
    match preset {
        ClonedPreset::Workspace(mut p) => {
            p.name = name;
            if overwrite {
                store.save_workspace_overwrite(p)
            } else {
                store.save_workspace(p)
            }
        }
        ClonedPreset::Tab(mut p) => {
            p.name = name;
            if overwrite {
                store.save_tab_overwrite(p)
            } else {
                store.save_tab(p)
            }
        }
        ClonedPreset::Pane(mut p) => {
            p.name = name;
            if overwrite {
                store.save_pane_overwrite(p)
            } else {
                store.save_pane(p)
            }
        }
    }
}

/// 이름이 충돌하면 overwrite에 따라 덮어쓰거나 SkippedExists를 반환한다.
/// explicit_name이 없으면 base_name으로 중복되지 않는 이름을 만든다.
pub fn save_inner(
    core: &crate::core::Core,
    base_name: &str,
    explicit_name: Option<&str>,
    overwrite: bool,
    preset: ClonedPreset,
) -> Result<SaveOutcome, PresetMutationError> {
    let mut store = crate::poison::recover_mutex(
        core.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    );

    let kind = preset.kind();
    let Some(name) = resolve_save_name(&store, kind, base_name, explicit_name, overwrite) else {
        return Ok(SaveOutcome::SkippedExists);
    };

    store_preset(&mut store, preset, name.clone(), overwrite)
        .map(|_| SaveOutcome::Saved(name))
        .map_err(PresetMutationError::Store)
}

pub fn delete_inner(
    core: &crate::core::Core,
    kind: PresetKind,
    name: &str,
) -> Result<(), PresetMutationError> {
    let mut store = crate::poison::recover_mutex(
        core.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    );
    store.delete(kind, name).map_err(PresetMutationError::Store)
}

pub fn rename_inner(
    core: &crate::core::Core,
    kind: PresetKind,
    from: &str,
    to: &str,
) -> Result<(), PresetMutationError> {
    let mut store = crate::poison::recover_mutex(
        core.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    );
    store
        .rename(kind, from, to)
        .map_err(PresetMutationError::Store)
}

/// kind에 맞는 워크스페이스·탭·pane ID로 프리셋을 캡처한다.
/// IPC preset.capture가 사용하며, UI는 캡처한 데이터를 SavePreset에 담는다.
pub fn capture_inner(
    presentation: &dyn crate::model::StructurePresentation,
    engine: &EngineRef<'_>,
    kind: PresetKind,
    source_id: u32,
) -> Result<(ClonedPreset, String), String> {
    let registry = engine.surface_registry.clone();

    match kind {
        PresetKind::Workspace => {
            let ws = engine
                .workspaces()
                .into_iter()
                .find(|w| w.id == source_id)
                .ok_or_else(|| format!("Workspace id {source_id} not found"))?;
            let base = if ws.name.is_empty() {
                "workspace".to_string()
            } else {
                ws.name.clone()
            };
            let preset = capture_workspace_preset(presentation, engine, ws, None, &registry)
                .ok_or_else(|| "workspace capture failed".to_string())?;
            Ok((ClonedPreset::Workspace(preset), base))
        }
        PresetKind::Tab => {
            let pane_id = engine
                .find_pane_for_tab(source_id)
                .ok_or_else(|| format!("Tab id {source_id} not found"))?;
            for ws in &engine.workspaces() {
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
                            let preset = capture_tab_preset(engine, tab, None, &registry)
                                .ok_or_else(|| "tab capture failed".to_string())?;
                            return Ok((ClonedPreset::Tab(preset), base));
                        }
                    }
                }
            }
            Err(format!("Tab id {source_id} not found"))
        }
        PresetKind::Pane => {
            for ws in &engine.workspaces() {
                if let Some(pane) = ws.pane_layout().find_pane(source_id) {
                    let preset = capture_pane_preset(presentation, engine, pane, None, &registry)
                        .ok_or_else(|| "pane capture failed".to_string())?;
                    return Ok((ClonedPreset::Pane(preset), "pane".to_string()));
                }
            }
            Err(format!("Pane id {source_id} not found"))
        }
    }
}

#[cfg(test)]
mod mirror_tests {
    use super::{PresetApplyTarget, PresetMutationError, apply_inner};
    use crate::state::preset_apply::{ApplyError, ApplyOptions};
    use tasty_presets::{
        PanePreset, PresetKind, PresetPane, PresetStore, PresetSurface, PresetSurfaceLayout,
        PresetTab, TabPreset,
    };

    /// PTY를 만들지 않도록 등록되지 않은 kind의 leaf를 쓴다.
    fn preset_tab() -> PresetTab {
        PresetTab {
            explicit_name: None,
            layout: PresetSurfaceLayout::Leaf {
                surface: PresetSurface {
                    id: None,
                    kind: "mirror-test-kind".into(),
                    cwd: None,
                    startup_command: None,
                    params: serde_json::Value::Null,
                },
            },
        }
    }

    fn core_with_presets(dir: &std::path::Path) -> crate::core::Core {
        let core = crate::ipc::handler::cli_entry_tests::test_core();
        let mut store = PresetStore::load_from(dir.into());
        store
            .save_tab(TabPreset {
                name: "t".into(),
                tab: preset_tab(),
            })
            .expect("save tab preset");
        store
            .save_pane(PanePreset {
                name: "p".into(),
                pane: PresetPane {
                    tabs: vec![preset_tab()],
                    active_tab: 0,
                },
            })
            .expect("save pane preset");
        *core.preset_store.lock().expect("preset store") = store;
        core
    }

    fn target(kind: PresetKind, pane: Option<u32>, ws: Option<u32>) -> PresetApplyTarget<'static> {
        PresetApplyTarget {
            kind,
            name: if kind == PresetKind::Tab { "t" } else { "p" },
            target_pane_id: pane,
            target_workspace_id: ws,
            category: None,
        }
    }

    fn is_mirror_refusal(err: &PresetMutationError) -> bool {
        matches!(
            err,
            PresetMutationError::Apply(ApplyError::Other(e))
                if e.downcast_ref::<crate::core::MirrorStructuralBlocked>()
                    .is_some_and(|b| !b.forwarded)
        ) && err.is_mirror_refusal()
    }

    #[test]
    fn tab_and_pane_presets_are_refused_on_a_mirror_workspace() {
        let dir = tempfile::tempdir().expect("tmp");
        let core = core_with_presets(dir.path());
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        engine.make_mirror_fixture(0);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .first_pane()
            .unwrap()
            .id;
        let panes_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()
            .len();
        let tabs_before = |engine: &crate::core::CoreState| {
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .find_pane(pane_id)
                .map(|p| p.tabs.len())
        };
        let tabs0 = tabs_before(&engine);
        let opts = || ApplyOptions { focus: false };

        // 대상을 지정한 경우와 활성 workspace로 해석하는 경우를 모두 확인한다.
        for t in [
            target(PresetKind::Tab, Some(pane_id), None),
            target(PresetKind::Tab, None, None),
            target(PresetKind::Pane, None, Some(ws_id)),
            target(PresetKind::Pane, None, None),
        ] {
            let kind = t.kind;
            let res = apply_inner(&core, &mut state, &mut engine, t, opts());
            let err = match res {
                Ok(_) => panic!("{kind:?}: mirror workspace에 preset이 적용됐다"),
                Err(e) => e,
            };
            assert!(is_mirror_refusal(&err), "{kind:?}: {err}");
        }
        assert_eq!(tabs_before(&engine), tabs0, "탭이 추가됐다");
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            panes_before,
            "pane이 추가됐다"
        );
    }

    #[test]
    fn tab_and_pane_presets_still_apply_on_a_local_workspace() {
        let dir = tempfile::tempdir().expect("tmp");
        let core = core_with_presets(dir.path());
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .first_pane()
            .unwrap()
            .id;

        for t in [
            target(PresetKind::Tab, Some(pane_id), None),
            target(PresetKind::Pane, None, Some(ws_id)),
        ] {
            let kind = t.kind;
            let res = apply_inner(
                &core,
                &mut state,
                &mut engine,
                t,
                ApplyOptions { focus: false },
            );
            assert!(
                res.is_ok(),
                "{kind:?}: {:?}",
                res.err().map(|e| e.to_string())
            );
        }
    }
}
