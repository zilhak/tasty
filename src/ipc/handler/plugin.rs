//! Plugin IPC handlers — `App`이 `PluginManager`를 들고 있으므로 일반 핸들러 라우팅
//! (`&mut AppState`)와 별도로, `App::process_ipc`에서 직접 호출된다.

use serde_json::{json, Value};

use crate::ipc::protocol::JsonRpcResponse;
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
