//! `preset.*` IPC 핸들러 — layout preset CRUD + apply.
//!
//! 모든 mutation 은 `crate::intent::preset` 의 공유 inner 함수를 호출한다.
//! Intent 큐를 거치지 않는 이유는 IPC 응답이 sync contract (성공/실패 + 결과 data)
//! 이기 때문 — 두 경로 모두 같은 inner 를 호출해 동작 일관성을 보장한다.
//!
//! CLI/IPC 경로는 포커스 독립 — `preset.apply` 는 항상 `ApplyOptions { focus: false }`.

use serde_json::json;
use tasty_presets::{PanePreset, PresetKind, TabPreset, WorkspacePreset};

use crate::intent::ClonedPreset;
use crate::intent::preset::{
    ApplyOutcome, PresetMutationError, SaveOutcome, apply_inner, capture_inner, delete_inner,
    rename_inner, save_inner,
};
use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;
use crate::state::preset_apply::ApplyOptions;

/// kind 문자열 → PresetKind. 잘못된 값이면 invalid_params 응답을 반환.
fn parse_kind(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<PresetKind, JsonRpcResponse> {
    let s = params
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "Missing 'kind' parameter"))?;
    match s {
        "workspace" => Ok(PresetKind::Workspace),
        "tab" => Ok(PresetKind::Tab),
        "pane" => Ok(PresetKind::Pane),
        other => Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("Invalid 'kind' value '{other}' (expected workspace|tab|pane)"),
        )),
    }
}

fn require_str<'a>(
    params: &'a serde_json::Value,
    key: &str,
    id: &serde_json::Value,
) -> Result<&'a str, JsonRpcResponse> {
    params.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        JsonRpcResponse::invalid_params(id.clone(), format!("Missing '{key}' parameter"))
    })
}

fn require_u32(
    params: &serde_json::Value,
    key: &str,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), format!("Missing '{key}' parameter"))
        })
}

/// `engine.preset_store` 가 None 이면 internal_error.
fn with_store<R>(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: &serde_json::Value,
    f: impl FnOnce(&tasty_presets::PresetStore) -> R,
) -> Result<R, JsonRpcResponse> {
    let arc = engine
        .preset_store
        .as_ref()
        .ok_or_else(|| JsonRpcResponse::internal_error(id.clone(), "preset_store unavailable"))?;
    let guard = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
    Ok(f(&guard))
}

/// PresetMutationError → JsonRpcResponse 매핑.
fn mutation_error(id: serde_json::Value, e: PresetMutationError) -> JsonRpcResponse {
    match &e {
        PresetMutationError::StoreUnavailable => JsonRpcResponse::internal_error(id, e.to_string()),
        PresetMutationError::NotFound { .. }
        | PresetMutationError::Apply(_)
        | PresetMutationError::Store(_) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

// ── handlers ──────────────────────────────────────────────────────────

pub fn handle_list(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let names = match with_store(state, engine, &id, |s| s.list(kind)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    JsonRpcResponse::success(id, json!({ "kind": kind.as_str(), "presets": names }))
}

pub fn handle_get(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };

    let data = match with_store(
        state,
        engine,
        &id,
        |s| -> Result<serde_json::Value, String> {
            match kind {
                PresetKind::Workspace => s
                    .get_workspace(&name)
                    .map(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
                    .ok_or_else(|| format!("preset not found: workspace/{name}"))?,
                PresetKind::Tab => s
                    .get_tab(&name)
                    .map(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
                    .ok_or_else(|| format!("preset not found: tab/{name}"))?,
                PresetKind::Pane => s
                    .get_pane(&name)
                    .map(|p| serde_json::to_value(p).map_err(|e| e.to_string()))
                    .ok_or_else(|| format!("preset not found: pane/{name}"))?,
            }
        },
    ) {
        Ok(Ok(v)) => v,
        Ok(Err(msg)) => return JsonRpcResponse::invalid_params(id, msg),
        Err(e) => return e,
    };

    JsonRpcResponse::success(
        id,
        json!({
            "kind": kind.as_str(),
            "name": name,
            "data": data,
        }),
    )
}

pub fn handle_save(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let data = match params.get("data") {
        Some(v) => v.clone(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'data' parameter"),
    };
    let overwrite = params
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 1. data → ClonedPreset 변환.
    let cloned = match kind {
        PresetKind::Workspace => match serde_json::from_value::<WorkspacePreset>(data) {
            Ok(p) => ClonedPreset::Workspace(p),
            Err(e) => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("invalid workspace preset: {e}"),
                );
            }
        },
        PresetKind::Tab => match serde_json::from_value::<TabPreset>(data) {
            Ok(p) => ClonedPreset::Tab(p),
            Err(e) => {
                return JsonRpcResponse::invalid_params(id, format!("invalid tab preset: {e}"));
            }
        },
        PresetKind::Pane => match serde_json::from_value::<PanePreset>(data) {
            Ok(p) => ClonedPreset::Pane(p),
            Err(e) => {
                return JsonRpcResponse::invalid_params(id, format!("invalid pane preset: {e}"));
            }
        },
    };

    // 2. 공유 save_inner 호출.
    match save_inner(state, engine, "", Some(&name), overwrite, cloned) {
        Ok(SaveOutcome::Saved(saved_name)) => {
            JsonRpcResponse::success(id, json!({ "name": saved_name }))
        }
        Ok(SaveOutcome::SkippedExists) => JsonRpcResponse::invalid_params(
            id,
            format!("preset '{name}' already exists (overwrite=false)"),
        ),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_delete(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };

    match delete_inner(state, engine, kind, &name) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "deleted": true })),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_rename(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let from = match require_str(params, "from", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let to = match require_str(params, "to", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };

    match rename_inner(state, engine, kind, &from, &to) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "renamed": to })),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_capture(
    state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let source_id = match require_u32(params, "source_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let explicit_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // 1. capture (read-only on engine).
    let (cloned, base_name) = match capture_inner(state, engine, kind, source_id) {
        Ok(v) => v,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    };

    // 2. save (overwrite=false; explicit_name=Some 이면 충돌 시 SkippedExists).
    match save_inner(
        state,
        engine,
        &base_name,
        explicit_name.as_deref(),
        false,
        cloned,
    ) {
        Ok(SaveOutcome::Saved(name)) => JsonRpcResponse::success(id, json!({ "name": name })),
        Ok(SaveOutcome::SkippedExists) => JsonRpcResponse::invalid_params(
            id,
            "preset name already exists (overwrite=false)".to_string(),
        ),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_apply(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let name = match require_str(params, "name", &id) {
        Ok(s) => s.to_string(),
        Err(e) => return e,
    };
    let target_pane_id = params
        .get("target_pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let target_workspace_id = params
        .get("target_workspace_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    // CLI/IPC 포커스 독립 원칙 — focus 항상 false.
    let opts = ApplyOptions { focus: false };
    match apply_inner(
        state,
        engine,
        kind,
        &name,
        target_pane_id,
        target_workspace_id,
        opts,
    ) {
        Ok(ApplyOutcome::Workspace { workspace_id }) => JsonRpcResponse::success(
            id,
            json!({
                "applied": true,
                "kind": "workspace",
                "workspace_id": workspace_id,
            }),
        ),
        Ok(ApplyOutcome::Tab { tab_id }) => JsonRpcResponse::success(
            id,
            json!({
                "applied": true,
                "kind": "tab",
                "tab_id": tab_id,
            }),
        ),
        Ok(ApplyOutcome::Pane { pane_id }) => JsonRpcResponse::success(
            id,
            json!({
                "applied": true,
                "kind": "pane",
                "pane_id": pane_id,
            }),
        ),
        Err(PresetMutationError::Apply(e)) => JsonRpcResponse::internal_error(id, e.to_string()),
        Err(e) => mutation_error(id, e),
    }
}
