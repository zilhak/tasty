//! Plugin IPC handlers — `App`이 `PluginManager`를 들고 있으므로 일반 핸들러 라우팅
//! (`&mut AppState`)와 별도로, `App::process_ipc`에서 직접 호출된다.

use serde_json::{json, Value};

use crate::ipc::protocol::JsonRpcResponse;
use crate::plugin::manifest::Permission;
use crate::plugin::{Manifest, PluginManager};

pub fn handle_list(mgr: Option<&PluginManager>, id: Value) -> JsonRpcResponse {
    let arr: Vec<Value> = match mgr {
        Some(mgr) => mgr
            .packages
            .iter()
            .map(|p| {
                json!({
                    "id": p.manifest.id,
                    "name": p.manifest.name,
                    "version": p.manifest.version,
                    "description": p.manifest.description,
                    "enabled": !mgr.config.is_disabled(&p.manifest.id),
                    "running": mgr.is_running(&p.manifest.id),
                    "surface_kinds": p.manifest.surface_kinds.iter().map(|k| &k.kind).collect::<Vec<_>>(),
                    "log_path": mgr.log_path(&p.manifest.id).to_string_lossy(),
                })
            })
            .collect(),
        None => Vec::new(),
    };
    JsonRpcResponse::success(id, json!({ "plugins": arr }))
}

pub fn handle_show(
    mgr: Option<&PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let pkg = match mgr.packages.iter().find(|p| p.manifest.id == plugin_id) {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                -32003,
                &format!("plugin '{plugin_id}' not installed"),
            );
        }
    };
    let manifest = &pkg.manifest;

    let granted: Vec<String> = mgr
        .config
        .granted_permissions(&plugin_id)
        .into_iter()
        .collect();

    let surface_kinds: Vec<Value> = manifest
        .surface_kinds
        .iter()
        .map(|k| {
            json!({
                "kind": k.kind,
                "display_name_i18n_key": k.display_name_i18n_key,
                "icon": k.icon,
                "rendering": format!("{:?}", k.rendering).to_lowercase(),
            })
        })
        .collect();

    let events_emitted: Vec<Value> = manifest
        .events_emitted
        .iter()
        .map(|e| {
            json!({
                "key": e.key,
                "description": e.description,
                "stability": format!("{:?}", e.stability).to_lowercase(),
                "payload_schema": e.payload_schema,
            })
        })
        .collect();

    let commands: Vec<Value> = manifest
        .contributes
        .commands
        .iter()
        .map(|c| {
            let override_repr =
                mgr.config.shortcut_override(&plugin_id, &c.id).map(|ov| match ov {
                    crate::plugin::registry_state::ShortcutOverride::Key { value } => {
                        json!({ "mode": "key", "value": value })
                    }
                    crate::plugin::registry_state::ShortcutOverride::Inherit { source } => {
                        json!({ "mode": "inherit", "source": source })
                    }
                    crate::plugin::registry_state::ShortcutOverride::None => {
                        json!({ "mode": "none" })
                    }
                });
            json!({
                "id": c.id,
                "title_i18n_key": c.title_i18n_key,
                "scope": format!("{:?}", c.scope).to_lowercase(),
                "binding_mode": format!("{:?}", c.binding_mode).to_lowercase(),
                "default_keybinding": c.default_keybinding,
                "shortcut_override": override_repr,
            })
        })
        .collect();

    let menu_items: Vec<Value> = manifest
        .contributes
        .menu_items
        .iter()
        .map(|m| json!({ "menu": m.menu, "command": m.command, "when": m.when }))
        .collect();

    let ipc_namespace: Vec<Value> = manifest
        .contributes
        .ipc_namespace
        .iter()
        .map(|n| json!({ "prefix": n.prefix }))
        .collect();

    let extension_state =
        mgr.extensions
            .state(&plugin_id)
            .map(|s| extension_state_to_json(s));

    let extends = manifest.extends.as_ref().map(|d| {
        let to_event_hook = |h: &crate::plugin::manifest::EventHookDecl| {
            json!({
                "event": h.event,
                "modifies": h.modifies,
                "mode": format!("{:?}", h.mode).to_lowercase(),
                "timeout_ms": h.timeout_ms,
            })
        };
        let to_ipc_hook = |h: &crate::plugin::manifest::IpcHookDecl| {
            json!({
                "method": h.method,
                "modifies": h.modifies,
                "mode": format!("{:?}", h.mode).to_lowercase(),
                "timeout_ms": h.timeout_ms,
            })
        };
        json!({
            "plugin_id": d.plugin_id,
            "version_req": d.version_req,
            "api_version": d.api_version,
            "pre_event": d.pre_event.iter().map(to_event_hook).collect::<Vec<_>>(),
            "post_event": d.post_event.iter().map(to_event_hook).collect::<Vec<_>>(),
            "pre_ipc": d.pre_ipc.iter().map(to_ipc_hook).collect::<Vec<_>>(),
            "post_ipc": d.post_ipc.iter().map(to_ipc_hook).collect::<Vec<_>>(),
        })
    });

    let cli: Vec<Value> = manifest
        .contributes
        .cli
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "description": c.description,
                "subcommands": c.subcommands.iter().map(|s| {
                    json!({ "name": s.name, "ipc_method": s.ipc_method, "description": s.description })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    JsonRpcResponse::success(
        id,
        json!({
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "description": manifest.description,
            "authors": manifest.authors,
            "homepage": manifest.homepage,
            "api_version": manifest.api_version,
            "manifest_version": manifest.manifest_version,
            "dir": pkg.dir.to_string_lossy(),
            "enabled": !mgr.config.is_disabled(&plugin_id),
            "running": mgr.is_running(&plugin_id),
            "log_path": mgr.log_path(&plugin_id).to_string_lossy(),
            "permissions": {
                "manifest": manifest.permissions,
                "granted": granted,
            },
            "event_subscribe": manifest.event_subscribe,
            "event_publish": manifest.event_publish,
            "events_emitted": events_emitted,
            "surface_kinds": surface_kinds,
            "commands": commands,
            "menu_items": menu_items,
            "ipc_namespace": ipc_namespace,
            "cli": cli,
            "extends": extends,
            "extension_state": extension_state,
        }),
    )
}

/// `plugin.extension.list` — 모든 extension의 현재 상태를 반환.
/// `[extends]` 블록이 없는 plugin은 결과에 포함되지 않는다.
pub fn handle_extension_list(mgr: Option<&PluginManager>, id: Value) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let mut entries: Vec<Value> = mgr
        .extensions
        .iter()
        .map(|(ext_id, state)| {
            let target_id = mgr
                .packages
                .iter()
                .find(|p| p.manifest.id == ext_id)
                .and_then(|p| p.manifest.extends.as_ref().map(|e| e.plugin_id.clone()));
            json!({
                "extension_id": ext_id,
                "target_id": target_id,
                "state": extension_state_to_json(state),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.get("extension_id")
            .and_then(|v| v.as_str())
            .cmp(&b.get("extension_id").and_then(|v| v.as_str()))
    });
    JsonRpcResponse::success(id, json!({ "extensions": entries }))
}

fn extension_state_to_json(state: &crate::plugin::extension_registry::ExtensionState) -> Value {
    use crate::plugin::extension_registry::{ExtensionState, PendingReason};
    match state {
        ExtensionState::Active {
            target_id,
            target_version,
        } => json!({
            "status": "active",
            "target_id": target_id,
            "target_version": target_version,
        }),
        ExtensionState::Pending(reason) => {
            let r = match reason {
                PendingReason::TargetMissing => json!({ "kind": "target_missing" }),
                PendingReason::TargetDisabled => json!({ "kind": "target_disabled" }),
                PendingReason::VersionMismatch {
                    target_version,
                    required,
                } => json!({
                    "kind": "version_mismatch",
                    "target_version": target_version,
                    "required": required,
                }),
                PendingReason::InvalidTargetVersion { target_version } => json!({
                    "kind": "invalid_target_version",
                    "target_version": target_version,
                }),
                PendingReason::PermissionNotGranted => {
                    json!({ "kind": "permission_not_granted" })
                }
            };
            json!({ "status": "pending", "reason": r })
        }
        ExtensionState::Disabled => json!({ "status": "disabled" }),
        ExtensionState::Conflict { other_extension_id } => json!({
            "status": "conflict",
            "other_extension_id": other_extension_id,
        }),
    }
}

pub fn handle_install(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => {
            return JsonRpcResponse::error(
                id,
                -32000,
                "plugin manager not initialized (no main window yet)",
            );
        }
    };
    let src_path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => std::path::PathBuf::from(p),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'path' parameter"),
    };
    let manifest = match Manifest::load(&src_path) {
        Ok(m) => m,
        Err(e) => {
            return JsonRpcResponse::error(id, -32001, &format!("invalid plugin at source: {e}"));
        }
    };
    let dest_root = match crate::plugin::plugin_root() {
        Some(r) => r,
        None => return JsonRpcResponse::error(id, -32000, "could not resolve plugins directory"),
    };
    let dest = dest_root.join(&manifest.id);
    if dest.exists() {
        return JsonRpcResponse::error(
            id,
            -32002,
            &format!("plugin '{}' already installed at {}", manifest.id, dest.display()),
        );
    }
    if let Err(e) = std::fs::create_dir_all(&dest_root) {
        return JsonRpcResponse::error(id, -32000, &format!("create dir failed: {e}"));
    }
    if let Err(e) = copy_dir_recursive(&src_path, &dest) {
        return JsonRpcResponse::error(id, -32000, &format!("copy failed: {e}"));
    }
    // discover + try to start the new plugin
    mgr.packages = crate::plugin::discovery::discover();
    // 새 plugin의 단축키 command 등록 (설정 UI/단축키 매칭이 즉시 인식)
    mgr.command_registry.register_plugin(&manifest);
    // extension 상태 재계산 — 새 plugin이 extension이거나 다른 extension의 target일 수 있음.
    mgr.recompute_extensions();
    // i18n namespace 등록 — 설치 직후부터 plugin 키 번역이 동작하도록
    let lang_dir = dest.join(&manifest.lang_dir);
    tasty_core::i18n::register_namespace(&manifest.id, &lang_dir);
    // 첫 설치 시: 매니페스트의 모든 권한을 grant (CLI 기반 install은 사용자가
    // 직접 명령을 실행했으므로 동의로 간주). GUI install 흐름이 추가되면 이
    // 자동 grant는 빼고 동의 모달에서 결정한다.
    let tokens: Vec<String> = manifest.permissions.clone();
    mgr.config.set_granted(&manifest.id, tokens);
    if let Err(e) = mgr.config.save() {
        tracing::warn!("plugins.toml save failed: {e}");
    }
    if !mgr.config.is_disabled(&manifest.id) {
        if let Err(e) = mgr.enable(&manifest.id) {
            return JsonRpcResponse::error(id, -32000, &format!("enable after install failed: {e}"));
        }
    }
    JsonRpcResponse::success(id, json!({ "installed": manifest.id, "path": dest.to_string_lossy() }))
}

pub fn handle_remove(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    // graceful shutdown first
    if let Err(e) = mgr.disable(&plugin_id) {
        tracing::warn!("disable before remove failed: {e}");
    }
    let plugin_dir = match crate::plugin::plugin_root() {
        Some(r) => r.join(&plugin_id),
        None => return JsonRpcResponse::error(id, -32000, "could not resolve plugins directory"),
    };
    if !plugin_dir.exists() {
        return JsonRpcResponse::error(
            id,
            -32003,
            &format!("plugin '{plugin_id}' not installed"),
        );
    }
    if let Err(e) = std::fs::remove_dir_all(&plugin_dir) {
        return JsonRpcResponse::error(id, -32000, &format!("remove dir failed: {e}"));
    }
    mgr.packages.retain(|p| p.manifest.id != plugin_id);
    mgr.command_registry.unregister_plugin(&plugin_id);
    tasty_core::i18n::unregister_namespace(&plugin_id);
    // extension 상태 재계산 — 제거된 plugin이 다른 extension의 target이었으면 Pending으로 강등.
    mgr.recompute_extensions();
    JsonRpcResponse::success(id, json!({ "removed": plugin_id }))
}

pub fn handle_enable(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    match mgr.enable(&plugin_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "enabled": plugin_id })),
        Err(e) => JsonRpcResponse::error(id, -32000, &format!("enable failed: {e}")),
    }
}

pub fn handle_disable(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    match mgr.disable(&plugin_id) {
        Ok(()) => JsonRpcResponse::success(id, json!({ "disabled": plugin_id })),
        Err(e) => JsonRpcResponse::error(id, -32000, &format!("disable failed: {e}")),
    }
}

pub fn handle_permissions(
    mgr: Option<&PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let pkg = match mgr.packages.iter().find(|p| p.manifest.id == plugin_id) {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                -32003,
                &format!("plugin '{plugin_id}' not installed"),
            );
        }
    };
    let manifest_perms: Vec<&str> = pkg
        .manifest
        .permissions
        .iter()
        .map(|s| s.as_str())
        .collect();
    let granted: Vec<String> = mgr
        .config
        .granted_permissions(&plugin_id)
        .into_iter()
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "id": plugin_id,
            "manifest": manifest_perms,
            "granted": granted,
        }),
    )
}

pub fn handle_grant(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let token = match params.get("permission").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'permission' parameter"),
    };
    if Permission::from_token(&token).is_none() {
        return JsonRpcResponse::error(id, -32001, &format!("unknown permission '{token}'"));
    }
    let pkg = match mgr.packages.iter().find(|p| p.manifest.id == plugin_id) {
        Some(p) => p.clone(),
        None => {
            return JsonRpcResponse::error(
                id,
                -32003,
                &format!("plugin '{plugin_id}' not installed"),
            );
        }
    };
    if !pkg.manifest.permissions.iter().any(|p| p == &token) {
        return JsonRpcResponse::error(
            id,
            -32002,
            &format!(
                "plugin '{plugin_id}' does not declare permission '{token}' in its manifest"
            ),
        );
    }
    let added = mgr.config.grant(&plugin_id, &token);
    if let Err(e) = mgr.config.save() {
        tracing::warn!("plugins.toml save failed: {e}");
    }
    refresh_plugin_permissions(mgr, &plugin_id);
    if token.starts_with("ext:") {
        mgr.recompute_extensions();
    }
    JsonRpcResponse::success(
        id,
        json!({ "id": plugin_id, "permission": token, "added": added }),
    )
}

pub fn handle_revoke(
    mgr: Option<&mut PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let token = match params.get("permission").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'permission' parameter"),
    };
    let removed = mgr.config.revoke(&plugin_id, &token);
    if let Err(e) = mgr.config.save() {
        tracing::warn!("plugins.toml save failed: {e}");
    }
    refresh_plugin_permissions(mgr, &plugin_id);
    if token.starts_with("ext:") {
        mgr.recompute_extensions();
    }
    JsonRpcResponse::success(
        id,
        json!({ "id": plugin_id, "permission": token, "removed": removed }),
    )
}

/// 매니페스트 + granted를 다시 교집합하여 manager의 in-memory 권한 set을 갱신.
fn refresh_plugin_permissions(mgr: &mut PluginManager, plugin_id: &str) {
    let Some(pkg) = mgr.packages.iter().find(|p| p.manifest.id == plugin_id).cloned() else {
        return;
    };
    let granted = mgr.config.granted_permissions(plugin_id);
    let perms: std::collections::HashSet<Permission> = pkg
        .manifest
        .parsed_permissions()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| granted.contains(&p.as_token()))
        .collect();
    mgr.set_plugin_permissions(plugin_id, perms);
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}
