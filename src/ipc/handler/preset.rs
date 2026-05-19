//! `preset.*` IPC 핸들러 — layout preset CRUD + apply.
//!
//! CLI/IPC 경로는 포커스 독립 — `preset.apply` 는 항상 `ApplyOptions { focus: false }`.

use serde_json::json;
use tasty_presets::{
    CaptureOptions, CapturedSurfaceMeta, PanePreset, PresetKind, TabPreset, WorkspacePreset,
};

use crate::ipc::protocol::JsonRpcResponse;
use crate::model::Surface;
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
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
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

/// `state.engine.preset_store` 가 None 이면 internal_error.
fn with_store<R>(
    state: &AppState,
    id: &serde_json::Value,
    f: impl FnOnce(&tasty_presets::PresetStore) -> R,
) -> Result<R, JsonRpcResponse> {
    let arc = state
        .engine
        .preset_store
        .as_ref()
        .ok_or_else(|| {
            JsonRpcResponse::internal_error(id.clone(), "preset_store unavailable")
        })?;
    let guard = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
    Ok(f(&guard))
}

fn with_store_mut<R>(
    state: &AppState,
    id: &serde_json::Value,
    f: impl FnOnce(&mut tasty_presets::PresetStore) -> R,
) -> Result<R, JsonRpcResponse> {
    let arc = state
        .engine
        .preset_store
        .as_ref()
        .ok_or_else(|| {
            JsonRpcResponse::internal_error(id.clone(), "preset_store unavailable")
        })?;
    let mut guard = match arc.lock() {
        Ok(g) => g,
        Err(p) => {
            tracing::warn!("preset_store mutex poisoned; recovering");
            p.into_inner()
        }
    };
    Ok(f(&mut guard))
}

// ── handlers ──────────────────────────────────────────────────────────

pub fn handle_list(
    state: &AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let kind = match parse_kind(params, &id) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let names = match with_store(state, &id, |s| s.list(kind)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    JsonRpcResponse::success(id, json!({ "kind": kind.as_str(), "presets": names }))
}

pub fn handle_get(
    state: &AppState,
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

    let data = match with_store(state, &id, |s| -> Result<serde_json::Value, String> {
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

    let result: Result<(), String> = match with_store_mut(state, &id, |s| -> Result<(), String> {
        match kind {
            PresetKind::Workspace => {
                let mut preset: WorkspacePreset =
                    serde_json::from_value(data).map_err(|e| format!("invalid workspace preset: {e}"))?;
                preset.name = name.clone();
                if overwrite {
                    s.save_workspace_overwrite(preset).map_err(|e| e.to_string())
                } else {
                    s.save_workspace(preset).map_err(|e| e.to_string())
                }
            }
            PresetKind::Tab => {
                let mut preset: TabPreset =
                    serde_json::from_value(data).map_err(|e| format!("invalid tab preset: {e}"))?;
                preset.name = name.clone();
                if overwrite {
                    s.save_tab_overwrite(preset).map_err(|e| e.to_string())
                } else {
                    s.save_tab(preset).map_err(|e| e.to_string())
                }
            }
            PresetKind::Pane => {
                let mut preset: PanePreset =
                    serde_json::from_value(data).map_err(|e| format!("invalid pane preset: {e}"))?;
                preset.name = name.clone();
                if overwrite {
                    s.save_pane_overwrite(preset).map_err(|e| e.to_string())
                } else {
                    s.save_pane(preset).map_err(|e| e.to_string())
                }
            }
        }
    }) {
        Ok(r) => r,
        Err(e) => return e,
    };

    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "name": name })),
        Err(msg) => JsonRpcResponse::invalid_params(id, msg),
    }
}

pub fn handle_delete(
    state: &AppState,
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

    let result = match with_store_mut(state, &id, |s| s.delete(kind, &name)) {
        Ok(r) => r,
        Err(e) => return e,
    };

    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "deleted": true })),
        Err(e) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

pub fn handle_rename(
    state: &AppState,
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

    let result = match with_store_mut(state, &id, |s| s.rename(kind, &from, &to)) {
        Ok(r) => r,
        Err(e) => return e,
    };

    match result {
        Ok(()) => JsonRpcResponse::success(id, json!({ "renamed": to })),
        Err(e) => JsonRpcResponse::invalid_params(id, e.to_string()),
    }
}

pub fn handle_capture(
    state: &AppState,
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

    let registry = state.engine.surface_registry.clone();
    let mut capture = move |s: &dyn Surface| -> Option<CapturedSurfaceMeta> {
        let def = registry.get(s.kind())?;
        let params = (def.snapshot)(s)?;
        Some(CapturedSurfaceMeta {
            kind: s.kind().to_string(),
            params,
        })
    };

    // 1. preset 캡처 (필요한 src 를 찾아서 변환)
    enum Captured {
        Workspace(WorkspacePreset),
        Tab(TabPreset),
        Pane(PanePreset),
    }

    let (captured, base_name) = match kind {
        PresetKind::Workspace => {
            let ws = match state.engine.workspaces.iter().find(|w| w.id == source_id) {
                Some(w) => w,
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("Workspace id {source_id} not found"),
                    );
                }
            };
            let base = if ws.name.is_empty() {
                "workspace".to_string()
            } else {
                ws.name.clone()
            };
            match WorkspacePreset::from_workspace(ws, &mut capture, CaptureOptions::default()) {
                Some(p) => (Captured::Workspace(p), base),
                None => return JsonRpcResponse::internal_error(id, "workspace capture failed"),
            }
        }
        PresetKind::Tab => {
            // tab_id 로 검색.
            let pane_id = match state.find_pane_for_tab(source_id) {
                Some(p) => p,
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("Tab id {source_id} not found"),
                    );
                }
            };
            let mut found: Option<(TabPreset, String)> = None;
            'outer: for ws in &state.engine.workspaces {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if tab.id == source_id {
                            let base = tab
                                .explicit_name
                                .clone()
                                .unwrap_or_else(|| tab.name.clone());
                            let base = if base.is_empty() { "tab".to_string() } else { base };
                            let preset = match TabPreset::from_tab(
                                tab,
                                &mut capture,
                                CaptureOptions::default(),
                            ) {
                                Some(p) => p,
                                None => {
                                    return JsonRpcResponse::internal_error(
                                        id,
                                        "tab capture failed",
                                    );
                                }
                            };
                            found = Some((preset, base));
                            break 'outer;
                        }
                    }
                }
            }
            match found {
                Some((p, base)) => (Captured::Tab(p), base),
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("Tab id {source_id} not found"),
                    );
                }
            }
        }
        PresetKind::Pane => {
            let mut found: Option<PanePreset> = None;
            'outer: for ws in &state.engine.workspaces {
                if let Some(pane) = ws.pane_layout().find_pane(source_id) {
                    found = match PanePreset::from_pane(
                        pane,
                        &mut capture,
                        CaptureOptions::default(),
                    ) {
                        Some(p) => Some(p),
                        None => {
                            return JsonRpcResponse::internal_error(id, "pane capture failed");
                        }
                    };
                    break 'outer;
                }
            }
            match found {
                Some(p) => (Captured::Pane(p), "pane".to_string()),
                None => {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!("Pane id {source_id} not found"),
                    );
                }
            }
        }
    };

    // 2. store 에 저장 (unique_name 처리)
    let save_result: Result<String, String> =
        match with_store_mut(state, &id, |s| -> Result<String, String> {
            let name = match explicit_name {
                Some(n) => n,
                None => s.unique_name(kind, &base_name),
            };
            match captured {
                Captured::Workspace(mut p) => {
                    p.name = name.clone();
                    s.save_workspace(p).map_err(|e| e.to_string())?;
                }
                Captured::Tab(mut p) => {
                    p.name = name.clone();
                    s.save_tab(p).map_err(|e| e.to_string())?;
                }
                Captured::Pane(mut p) => {
                    p.name = name.clone();
                    s.save_pane(p).map_err(|e| e.to_string())?;
                }
            }
            Ok(name)
        }) {
            Ok(r) => r,
            Err(e) => return e,
        };

    match save_result {
        Ok(name) => JsonRpcResponse::success(id, json!({ "name": name })),
        Err(msg) => JsonRpcResponse::invalid_params(id, msg),
    }
}

pub fn handle_apply(
    state: &mut AppState,
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

    // 1. store 에서 clone (락 해제 전에)
    enum Cloned {
        Workspace(WorkspacePreset),
        Tab(TabPreset),
        Pane(PanePreset),
    }
    let cloned = match with_store(state, &id, |s| match kind {
        PresetKind::Workspace => s.get_workspace(&name).cloned().map(Cloned::Workspace),
        PresetKind::Tab => s.get_tab(&name).cloned().map(Cloned::Tab),
        PresetKind::Pane => s.get_pane(&name).cloned().map(Cloned::Pane),
    }) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("preset not found: {}/{name}", kind.as_str()),
            );
        }
        Err(e) => return e,
    };

    // 2. apply — focus 항상 false (CLI/IPC 포커스 독립 원칙)
    let opts = ApplyOptions { focus: false };
    match cloned {
        Cloned::Workspace(p) => match state.apply_workspace_preset(&p, opts) {
            Ok(idx) => {
                let ws_id = state.engine.workspaces[idx].id;
                JsonRpcResponse::success(
                    id,
                    json!({
                        "applied": true,
                        "kind": "workspace",
                        "workspace_id": ws_id,
                    }),
                )
            }
            Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
        },
        Cloned::Tab(p) => match state.apply_tab_preset(&p, target_pane_id, opts) {
            Ok(tab_id) => JsonRpcResponse::success(
                id,
                json!({
                    "applied": true,
                    "kind": "tab",
                    "tab_id": tab_id,
                }),
            ),
            Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
        },
        Cloned::Pane(p) => match state.apply_pane_preset(&p, target_workspace_id, opts) {
            Ok(pane_id) => JsonRpcResponse::success(
                id,
                json!({
                    "applied": true,
                    "kind": "pane",
                    "pane_id": pane_id,
                }),
            ),
            Err(e) => JsonRpcResponse::internal_error(id, e.to_string()),
        },
    }
}
