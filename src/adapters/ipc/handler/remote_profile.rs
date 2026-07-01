//! `remote.profile.*` IPC — 원격 접속 프로필 CRUD (원칙 2 보강).
//! (구 `tool.ssh.*` / `ssh.profile.*` 는 `crates/tasty-ipc/src/alias.rs` 에서 정규화되어 도달.)
//!
//! 소켓만 가진 에이전트도 프로필을 관리할 수 있게 CLI(`tasty tool ssh`)와 같은
//! `~/.tasty/remote-profiles.toml` 를 IPC 로도 노출한다. 프로필은 비밀을 담지 않고
//! passkey 를 이름으로 참조만 한다. 포커스 비의존(원칙 3): 대상을 `name` 으로 지정.

use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::{
    Passkeys, RemoteProfile, RemoteProfiles, is_valid_shell, sanitize_passkey_name,
    shell_to_port_mode,
};

fn profile_to_json(p: &RemoteProfile) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// `remote.profile.list` → 전 프로필 목록.
pub(crate) fn handle_list(id: Value) -> JsonRpcResponse {
    let profiles = RemoteProfiles::load();
    let arr: Vec<_> = profiles.profiles.iter().map(profile_to_json).collect();
    JsonRpcResponse::success(id, json!({ "profiles": arr }))
}

/// `remote.profile.get` { name } → 한 프로필.
pub(crate) fn handle_get(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let profiles = RemoteProfiles::load();
    match profiles.get(name) {
        Some(p) => JsonRpcResponse::success(id, json!({ "profile": profile_to_json(p) })),
        None => JsonRpcResponse::error(id, -32040, format!("remote profile '{name}' not found")),
    }
}

/// `remote.profile.add` → upsert.
///
/// 일반형: { name, kind?, label?, passkey_ref?, fields? } — **tasty-attach kind CRUD 는
/// 이 일반 fields 경로로 양면 노출된다**(예: kind="tasty-attach", fields={ssh_ref,
/// remote_tasty, port_mode, port_file}). ssh 편의형: kind=ssh 일 때 host/user/port/
/// identity_file/extra_options/shell 을 받아 fields/passkey 로 접는다(shell→port_mode
/// 도출). `identity_file` → path passkey.
pub(crate) fn handle_add(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let kind = params.get("kind").and_then(|v| v.as_str()).unwrap_or("ssh");
    let mut p = RemoteProfile::new(name, kind);
    p.label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // 제네릭 fields (스칼라/문자열 리스트).
    if let Some(obj) = params.get("fields").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                p.set_field(k.clone(), s.to_string());
            } else if let Some(arr) = v.as_array() {
                let list: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect();
                p.set_field(k.clone(), list);
            }
        }
    }

    let mut passkeys = Passkeys::load();
    let mut will_detect = false;

    if kind == "ssh" {
        // ssh 편의형 — 구 tool.ssh.add 호환.
        if let Some(host) = params.get("host").and_then(|v| v.as_str()) {
            p.set_field("host", host.to_string());
        }
        if p.as_ssh().and_then(|v| v.host()).is_none() {
            return JsonRpcResponse::invalid_params(id, "ssh kind requires 'host'");
        }
        if let Some(user) = params.get("user").and_then(|v| v.as_str()) {
            p.set_field("user", user.to_string());
        }
        if let Some(port) = params
            .get("port")
            .and_then(|v| v.as_u64())
            .filter(|v| *v <= u16::MAX as u64)
        {
            p.set_field("port", port.to_string());
        }
        if let Some(opts) = params.get("extra_options").and_then(|v| v.as_array()) {
            let list: Vec<String> = opts
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if !list.is_empty() {
                p.set_field("extra_options", list);
            }
        }
        if let Some(rt) = params.get("remote_tasty").and_then(|v| v.as_str()) {
            p.set_field("remote_tasty", rt.to_string());
        }
        if let Some(pm) = params.get("port_mode").and_then(|v| v.as_str()) {
            p.set_field("port_mode", pm.to_string());
        }
        let shell = params
            .get("shell")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        if !is_valid_shell(shell) {
            return JsonRpcResponse::invalid_params(
                id,
                "invalid 'shell' (powershell|cmd|bash|zsh|auto)",
            );
        }
        p.set_field("shell", shell.to_string());
        if let Some(mode) = shell_to_port_mode(shell) {
            p.set_field("port_mode", mode.to_string());
            p.remove_field("detect_failed");
        } else {
            will_detect = true; // auto → 등록 후 워커 감지
        }
    }

    // 자격증명: passkey_ref 직접 지정 우선, 아니면 identity_file → path passkey.
    if let Some(pr) = params.get("passkey_ref").and_then(|v| v.as_str()) {
        p.passkey_ref = Some(pr.to_string());
    } else if let Some(idf) = params.get("identity_file").and_then(|v| v.as_str()) {
        let pk_name = format!("{}-key", sanitize_passkey_name(name));
        if let Err(e) = passkeys.upsert_path(&pk_name, idf.to_string()) {
            return JsonRpcResponse::invalid_params(id, format!("invalid identity passkey: {e}"));
        }
        p.passkey_ref = Some(pk_name);
    }

    let mut profiles = RemoteProfiles::load();
    let replaced = profiles.get(name).is_some();
    profiles.upsert(p);
    if let Err(e) = passkeys.save() {
        return JsonRpcResponse::internal_error(id, format!("failed to save passkey: {e}"));
    }
    match profiles.save() {
        Ok(()) => {
            if will_detect {
                spawn_detect(name.to_string());
            }
            JsonRpcResponse::success(
                id,
                json!({ "saved": true, "name": name, "replaced": replaced, "detecting": will_detect }),
            )
        }
        Err(e) => {
            JsonRpcResponse::internal_error(id, format!("failed to save remote profile: {e}"))
        }
    }
}

/// `remote.profile.detect` { name } → 재감지(프로브 체인)를 워커 스레드에서 실행.
pub(crate) fn handle_detect(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let profiles = RemoteProfiles::load();
    if profiles.get(name).is_none() {
        return JsonRpcResponse::error(id, -32040, format!("remote profile '{name}' not found"));
    }
    spawn_detect(name.to_string());
    JsonRpcResponse::success(id, json!({ "detecting": true, "name": name }))
}

fn spawn_detect(name: String) {
    std::thread::spawn(move || match tasty_cli::ssh::detect_and_persist(&name) {
        Ok(mode) => tracing::info!("remote profile '{name}' 감지 성공 → {}", mode.as_str()),
        Err(e) => tracing::warn!("remote profile '{name}' 감지 실패(비활성): {e}"),
    });
}

/// `remote.profile.remove` { name } → 제거.
pub(crate) fn handle_remove(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let mut profiles = RemoteProfiles::load();
    if !profiles.remove(name) {
        return JsonRpcResponse::error(id, -32040, format!("remote profile '{name}' not found"));
    }
    match profiles.save() {
        Ok(()) => JsonRpcResponse::success(id, json!({ "removed": true, "name": name })),
        Err(e) => {
            JsonRpcResponse::internal_error(id, format!("failed to save remote profile: {e}"))
        }
    }
}
