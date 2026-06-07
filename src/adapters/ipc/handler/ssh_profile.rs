//! `ssh.profile.*` IPC — SSH 연결 프로필 CRUD (attach/detach 단계 7, 원칙 2 보강).
//!
//! 소켓만 가진 에이전트도 프로필을 관리할 수 있게 CLI(`tasty ssh-profile`)와 같은
//! `~/.tasty/ssh-profiles.toml` 를 IPC 로도 노출한다. 호스트는 프로필을 **저장만**
//! 하고 해석(→SSH 터널)은 자동 attach 경로(`src/app/auto_attach.rs`)가 흡수한다.
//! 포커스 비의존(원칙 3): 대상을 `name` 으로 직접 지정.

use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ssh_profiles::{SshProfile, SshProfiles};

fn profile_to_json(p: &SshProfile) -> serde_json::Value {
    serde_json::to_value(p).unwrap_or(serde_json::Value::Null)
}

/// `ssh.profile.list` → 전 프로필 목록.
pub(crate) fn handle_list(id: serde_json::Value) -> JsonRpcResponse {
    let profiles = SshProfiles::load();
    let arr: Vec<_> = profiles.profiles.iter().map(profile_to_json).collect();
    JsonRpcResponse::success(id, json!({ "profiles": arr }))
}

/// `ssh.profile.get` { name } → 한 프로필.
pub(crate) fn handle_get(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let profiles = SshProfiles::load();
    match profiles.get(name) {
        Some(p) => JsonRpcResponse::success(id, json!({ "profile": profile_to_json(p) })),
        None => JsonRpcResponse::error(id, -32040, format!("ssh profile '{name}' not found")),
    }
}

/// `ssh.profile.add` { name, host, user?, port?, identity_file?, use_agent?,
/// extra_options?, remote_tasty?, port_mode?, label? } → upsert.
pub(crate) fn handle_add(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let Some(host) = params.get("host").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'host' parameter");
    };
    let mut p = SshProfile::new(name, host);
    p.user = params
        .get("user")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    p.port = params
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|v| *v <= u16::MAX as u64)
        .map(|v| v as u16);
    p.identity_file = params
        .get("identity_file")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if let Some(a) = params.get("use_agent").and_then(|v| v.as_bool()) {
        p.use_agent = a;
    }
    if let Some(opts) = params.get("extra_options").and_then(|v| v.as_array()) {
        p.extra_options = opts
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(rt) = params.get("remote_tasty").and_then(|v| v.as_str()) {
        p.remote_tasty = rt.to_string();
    }
    if let Some(pm) = params.get("port_mode").and_then(|v| v.as_str()) {
        p.port_mode = pm.to_string();
    }
    p.label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let mut profiles = SshProfiles::load();
    let replaced = profiles.get(name).is_some();
    profiles.upsert(p);
    match profiles.save() {
        Ok(()) => JsonRpcResponse::success(
            id,
            json!({ "saved": true, "name": name, "replaced": replaced }),
        ),
        Err(e) => JsonRpcResponse::internal_error(id, format!("failed to save ssh profile: {e}")),
    }
}

/// `ssh.profile.remove` { name } → 제거.
pub(crate) fn handle_remove(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let mut profiles = SshProfiles::load();
    if !profiles.remove(name) {
        return JsonRpcResponse::error(id, -32040, format!("ssh profile '{name}' not found"));
    }
    match profiles.save() {
        Ok(()) => JsonRpcResponse::success(id, json!({ "removed": true, "name": name })),
        Err(e) => JsonRpcResponse::internal_error(id, format!("failed to save ssh profile: {e}")),
    }
}
