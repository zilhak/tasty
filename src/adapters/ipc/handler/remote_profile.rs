//! `remote.profile.*` IPC — 원격 접속 프로필 CRUD (원칙 2 보강).
//! (구 `tool.ssh.*` / `ssh.profile.*` 는 `crates/tasty-ipc/src/alias.rs` 에서 정규화되어 도달.)
//!
//! 소켓만 가진 에이전트도 프로필을 관리할 수 있게 CLI(`tasty tool ssh`)와 같은
//! `~/.tasty/remote-profiles.toml` 를 IPC 로도 노출한다. 프로필은 비밀을 담지 않고
//! passkey 를 이름으로 참조만 한다. 포커스 비의존(원칙 3): 대상을 `name` 으로 지정.

use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::{
    ImportError, Passkeys, RemoteProfile, RemoteProfiles, config_availability, enumerate_hosts,
    imported_as, is_valid_shell, prepare_import, sanitize_passkey_name, shell_to_port_mode,
    user_config_path,
};

/// 이 네임스페이스(원격 프로필 / passkey)가 쓰는 tasty 전용 에러 코드 블록은 `-3204x` 다.
/// 지금 쓰이는 건 아래 둘뿐이고 `-32042..-32049` 는 비어 있다.
///
/// **가리킨 대상이 없다.** get/remove/import 가 존재하지 않는 이름·alias 를 받았을 때.
const ERR_NOT_FOUND: i32 = -32040;

/// **이름이 이미 있다.** `import` 가 기존 프로필 이름과 충돌했을 때.
///
/// JSON-RPC 표준 `-32602`(invalid params) 가 아니다 — `-32602` 는 "파라미터를 고쳐서
/// 다시 보내라" 는 뜻이라 호출자가 요청을 뜯어보게 만들지만, 이름 충돌은 요청 자체는
/// 멀쩡하고 **저장소 상태**가 부딪힌 것이다. 호출자가 할 일도 다르다 — 다른 이름을 쓰거나,
/// 덮어쓰려면 `remote.profile.add`(upsert) 로 간다. 메시지 문자열을 파싱하지 않고
/// 코드만으로 이 분기를 잡을 수 있어야 한다.
const ERR_NAME_CONFLICT: i32 = -32041;

/// [`ImportError`] → 응답. 저장소를 읽지 않는 순수 매핑이라 홈 디렉토리 없이 테스트된다.
fn import_error_response(id: Value, err: ImportError) -> JsonRpcResponse {
    match err {
        ImportError::UnknownAlias(a) => JsonRpcResponse::error(
            id,
            ERR_NOT_FOUND,
            format!("ssh config alias '{a}' not found"),
        ),
        ImportError::NameTaken(n) => JsonRpcResponse::error(
            id,
            ERR_NAME_CONFLICT,
            format!("remote profile '{n}' exists"),
        ),
    }
}

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
        None => JsonRpcResponse::error(
            id,
            ERR_NOT_FOUND,
            format!("remote profile '{name}' not found"),
        ),
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
        return JsonRpcResponse::error(
            id,
            ERR_NOT_FOUND,
            format!("remote profile '{name}' not found"),
        );
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

/// `remote.profile.list_local` → 로컬 ssh config(`~/.ssh/config` + Include)의 Host alias.
///
/// 읽기 전용 · 프로세스 spawn 없음(`ssh -G` 는 `Match exec` 를 실제로 실행하므로 쓰지
/// 않는다). `hostname`/`user`/`port` 는 **표시 전용 hint** 라 프로필 저장에 쓰지 않는다.
/// `config_exists` / `config_readable` 은 **빈 `aliases` 의 이유**를 가른다. 셋 다 빈
/// 목록으로 떨어지지만 호출자가 할 일은 전부 다르다: 파일이 없으면(`exists:false`)
/// 만들라고, 있는데 못 읽으면(`exists:true, readable:false`) 권한을 고치라고, 둘 다
/// true 인데 비었으면 config 에 Host 가 없다고 안내해야 한다. GUI 는 in-process 라
/// 파일을 직접 보면 되지만 IPC 호출자에겐 응답에 실려야만 보인다.
/// `config_readable` 은 **최상위 파일 한정**이다(`Include` 는 검사하지 않는다 —
/// 못 읽은 include 는 코어가 warn 로그를 남긴다).
pub(crate) fn handle_list_local(id: Value) -> JsonRpcResponse {
    let profiles = RemoteProfiles::load();
    let aliases: Vec<Value> = enumerate_hosts()
        .into_iter()
        .map(|h| {
            json!({
                "name": h.alias,
                "source": h.source.display().to_string(),
                "hostname": h.hostname,
                "user": h.user,
                "port": h.port,
                "imported_as": imported_as(&profiles, &h.alias),
            })
        })
        .collect();
    let path = user_config_path();
    let avail = config_availability(path.as_deref());
    JsonRpcResponse::success(
        id,
        json!({
            "aliases": aliases,
            "config_path": path.as_ref().map(|p| p.display().to_string()),
            "config_exists": avail.exists,
            "config_readable": avail.readable,
        }),
    )
}

/// `remote.profile.import` { from, name, label? } → ssh config alias 를 프로필로 등록.
///
/// alias 문자열만 `host` 에 담는다 — `HostName`/`User`/`Port`/`ProxyJump` 를 펼쳐
/// 복사하면 ssh config 가 바뀔 때 값이 어긋난다(해석은 접속 시점의 ssh 가 한다).
/// **`handle_add` 와 달리 셸 자동 감지를 spawn 하지 않는다**: 감지는 실제 SSH 접속이라,
/// 목록에서 여러 건을 가져오면 접속이 연쇄로 일어난다. 감지는 `remote.profile.detect`.
pub(crate) fn handle_import(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(from) = params.get("from").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'from' parameter");
    };
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let label = params
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut profiles = RemoteProfiles::load();
    let profile = match prepare_import(&profiles, &enumerate_hosts(), from, name, label) {
        Ok(p) => p,
        Err(e) => return import_error_response(id, e),
    };
    profiles.upsert(profile);
    match profiles.save() {
        Ok(()) => JsonRpcResponse::success(
            id,
            json!({ "saved": true, "name": name, "from": from, "detecting": false }),
        ),
        Err(e) => {
            JsonRpcResponse::internal_error(id, format!("failed to save remote profile: {e}"))
        }
    }
}

/// `remote.profile.remove` { name } → 제거.
pub(crate) fn handle_remove(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let mut profiles = RemoteProfiles::load();
    if !profiles.remove(name) {
        return JsonRpcResponse::error(
            id,
            ERR_NOT_FOUND,
            format!("remote profile '{name}' not found"),
        );
    }
    match profiles.save() {
        Ok(()) => JsonRpcResponse::success(id, json!({ "removed": true, "name": name })),
        Err(e) => {
            JsonRpcResponse::internal_error(id, format!("failed to save remote profile: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 코드값은 계약이다 — 바꾸면 이 테스트가 먼저 깨져야 한다.
    #[test]
    fn import_error_codes_are_distinct_and_stable() {
        let unknown = import_error_response(json!(1), ImportError::UnknownAlias("nope".into()));
        let taken = import_error_response(json!(2), ImportError::NameTaken("dup".into()));
        let code = |r: &JsonRpcResponse| r.error.as_ref().expect("error 응답이어야 한다").code;
        assert_eq!(code(&unknown), -32040);
        assert_eq!(code(&taken), -32041);
        // 이름 충돌은 더 이상 표준 invalid_params 로 뭉뚱그리지 않는다.
        assert_ne!(code(&taken), -32602);
        assert!(taken.result.is_none());
    }
}
