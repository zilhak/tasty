//! `tool.ssh.*` IPC — SSH 연결 프로필 CRUD (attach/detach 단계 7, 원칙 2 보강).
//! (구 `ssh.profile.*` 는 `crates/tasty-ipc/src/alias.rs` 에서 정규화되어 도달.)
//!
//! 소켓만 가진 에이전트도 프로필을 관리할 수 있게 CLI(`tasty tool ssh`)와 같은
//! `~/.tasty/ssh-profiles.toml` 를 IPC 로도 노출한다. 호스트는 프로필을 **저장만**
//! 하고 해석(→SSH 터널)은 자동 attach 경로(`src/app/auto_attach.rs`)가 흡수한다.
//! 포커스 비의존(원칙 3): 대상을 `name` 으로 직접 지정.

use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ssh_profiles::{SshProfile, SshProfiles};

fn profile_to_json(p: &SshProfile) -> serde_json::Value {
    serde_json::to_value(p).unwrap_or(serde_json::Value::Null)
}

/// `tool.ssh.list` → 전 프로필 목록.
pub(crate) fn handle_list(id: serde_json::Value) -> JsonRpcResponse {
    let profiles = SshProfiles::load();
    let arr: Vec<_> = profiles.profiles.iter().map(profile_to_json).collect();
    JsonRpcResponse::success(id, json!({ "profiles": arr }))
}

/// `tool.ssh.get` { name } → 한 프로필.
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

/// `tool.ssh.add` { name, host, user?, port?, identity_file?, use_agent?,
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

    // shell (원칙 2 — CLI `--shell` 동등). 명시 셸은 발견 모드를 즉시 도출(블록 없음).
    // auto 는 등록 시 감지가 필요하나 SSH I/O 는 host 이벤트 루프를 막으므로 워커 스레드로
    // 분리한다(완료 시 toml 재저장). 잘못된 셸 값은 거부.
    if let Some(shell) = params.get("shell").and_then(|v| v.as_str()) {
        if !tasty_ssh_profiles::is_valid_shell(shell) {
            return JsonRpcResponse::invalid_params(
                id,
                "invalid 'shell' (powershell|cmd|bash|zsh|auto)",
            );
        }
        p.shell = shell.to_string();
    }
    if let Some(mode) = tasty_ssh_profiles::shell_to_port_mode(&p.shell) {
        // 명시 셸: 매핑으로 즉시 도출, 활성.
        p.port_mode = mode.to_string();
        p.detect_failed = false;
    }
    let will_detect = tasty_ssh_profiles::shell_to_port_mode(&p.shell).is_none();

    let mut profiles = SshProfiles::load();
    let replaced = profiles.get(name).is_some();
    profiles.upsert(p);
    match profiles.save() {
        Ok(()) => {
            // shell=auto → 등록 시 1회 감지를 워커 스레드에서 실행(host 무블록).
            if will_detect {
                spawn_detect(name.to_string());
            }
            JsonRpcResponse::success(
                id,
                json!({ "saved": true, "name": name, "replaced": replaced, "detecting": will_detect }),
            )
        }
        Err(e) => JsonRpcResponse::internal_error(id, format!("failed to save ssh profile: {e}")),
    }
}

/// `tool.ssh.detect` { name } → 재감지(프로브 체인)를 워커 스레드에서 실행하고 즉시
/// 응답한다(원칙 2 — CLI `tasty tool ssh detect` 동등). 완료 시 toml 이 갱신된다
/// (성공: 발견 모드 + 활성, 실패: detect_failed=true 비활성). host 이벤트 루프 무블록.
pub(crate) fn handle_detect(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let profiles = SshProfiles::load();
    if profiles.get(name).is_none() {
        return JsonRpcResponse::error(id, -32040, format!("ssh profile '{name}' not found"));
    }
    spawn_detect(name.to_string());
    JsonRpcResponse::success(id, json!({ "detecting": true, "name": name }))
}

/// 워커 스레드에서 `detect_and_persist` 실행(SSH 프로브 + toml 갱신). 결과는 toml 에
/// 반영되며, 호출자는 즉시 반환한다(host 이벤트 루프 무블록).
fn spawn_detect(name: String) {
    std::thread::spawn(move || match tasty_cli::ssh::detect_and_persist(&name) {
        Ok(mode) => tracing::info!("ssh profile '{name}' 감지 성공 → {}", mode.as_str()),
        Err(e) => tracing::warn!("ssh profile '{name}' 감지 실패(비활성): {e}"),
    });
}

/// `tool.ssh.remove` { name } → 제거.
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
