//! 프리셋 저장·변경·적용은 intent와 같은 공용 함수를 호출한다.
//! IPC는 성공/실패를 동기로 응답해야 하므로 intent 큐를 거치지 않는다.
//! 적용할 때 focus:false로 사용자 포커스를 유지한다.

use serde_json::json;
use tasty_presets::{PanePreset, PresetKind, TabPreset, WorkspacePreset};

use crate::intent::ClonedPreset;
use crate::intent::preset::{
    ApplyOutcome, PresetApplyTarget, PresetMutationError, SaveOutcome, capture_inner, delete_inner,
    rename_inner, save_inner,
};
use crate::state::preset_apply::ApplyOptions;
use tasty_ipc::protocol::JsonRpcResponse;

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

use super::params::require_u32;

fn with_store<R>(core: &crate::core::Core, f: impl FnOnce(&tasty_presets::PresetStore) -> R) -> R {
    let guard = crate::poison::recover_mutex(
        core.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    );
    f(&guard)
}

fn mutation_error(id: serde_json::Value, e: PresetMutationError) -> JsonRpcResponse {
    match &e {
        PresetMutationError::NotFound { .. }
        | PresetMutationError::Apply(_)
        | PresetMutationError::Store(_) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

pub fn handle_list(
    core: &crate::core::Core,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let names = with_store(core, |s| s.list(kind));
    JsonRpcResponse::success(id, json!({ "kind": kind.as_str(), "presets": names }))
}

pub fn handle_get(
    core: &crate::core::Core,
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

    let data = match with_store(core, |s| -> Result<serde_json::Value, String> {
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
    }) {
        Ok(v) => v,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
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
    core: &crate::core::Core,
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

    match save_inner(core, "", Some(&name), overwrite, cloned) {
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
    core: &crate::core::Core,
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

    match delete_inner(core, kind, &name) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "deleted": true })),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_rename(
    core: &crate::core::Core,
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

    match rename_inner(core, kind, &from, &to) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "renamed": to })),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_capture(
    core: &crate::core::Core,
    engine: &crate::core::CoreState,
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

    let (cloned, base_name) = match capture_inner(engine, kind, source_id) {
        Ok(v) => v,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    };

    match save_inner(core, &base_name, explicit_name.as_deref(), false, cloned) {
        Ok(SaveOutcome::Saved(name)) => JsonRpcResponse::success(id, json!({ "name": name })),
        Ok(SaveOutcome::SkippedExists) => JsonRpcResponse::invalid_params(
            id,
            "preset name already exists (overwrite=false)".to_string(),
        ),
        Err(e) => mutation_error(id, e),
    }
}

pub fn handle_apply(
    core: &crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
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
    let target_pane_id = match super::params::optional_u32(params, "target_pane_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let target_workspace_id = match super::params::optional_u32(params, "target_workspace_id", &id)
    {
        Ok(v) => v,
        Err(e) => return e,
    };

    let opts = ApplyOptions { focus: false };
    match window.apply_preset(
        core,
        engine,
        PresetApplyTarget {
            kind,
            name: &name,
            target_pane_id,
            target_workspace_id,
            // 카테고리는 UI 메뉴의 임시 상태이며 이 API에서는 지정하지 않는다.
            category: None,
        },
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
