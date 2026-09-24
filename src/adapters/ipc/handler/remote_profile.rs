//! 원격 프로필을 이름으로 관리한다. 비밀값은 담지 않고 passkey 이름으로 참조한다.
//! 이전 tool.ssh/ssh.profile 메서드는 alias 정규화를 거쳐 들어온다.

use super::params::{self, p_try};
use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::{
    ImportError, Passkeys, RemoteProfile, RemoteProfiles, config_availability, enumerate_hosts,
    imported_as, is_valid_shell, prepare_import, sanitize_passkey_name, shell_to_port_mode,
    user_config_path,
};

/// 존재하지 않는 프로필 이름 또는 SSH alias.
const ERR_NOT_FOUND: i32 = -32040;

/// import 이름 충돌. 다른 이름을 쓰거나 add의 upsert로 덮어쓸 수 있다.
const ERR_NAME_CONFLICT: i32 = -32041;

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

pub(crate) fn handle_list(id: Value) -> JsonRpcResponse {
    let profiles = RemoteProfiles::load();
    let arr: Vec<_> = profiles.profiles.iter().map(profile_to_json).collect();
    JsonRpcResponse::success(id, json!({ "profiles": arr }))
}

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

/// 일반 fields로 프로필을 추가·수정한다. tasty-attach도 같은 입력을 쓴다.
/// ssh의 host/user/port/identity_file/extra_options/shell 편의 인자는 fields/passkey로 변환한다.
/// identity_file은 path passkey이고 shell에서 port_mode를 도출한다.
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
        if let Some(host) = params.get("host").and_then(|v| v.as_str()) {
            p.set_field("host", host.to_string());
        }
        if p.as_ssh().and_then(|v| v.host()).is_none() {
            return JsonRpcResponse::invalid_params(id, "ssh kind requires 'host'");
        }
        if let Some(user) = params.get("user").and_then(|v| v.as_str()) {
            p.set_field("user", user.to_string());
        }
        if let Some(port) =
            p_try!(params::opt_int::<u64>(params, "port", &id)).filter(|v| *v <= u16::MAX as u64)
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
    std::thread::spawn(move || match tasty_ssh::detect_and_persist(&name) {
        Ok(mode) => tracing::info!("remote profile '{name}' 감지 성공 → {}", mode.as_str()),
        Err(e) => tracing::warn!("remote profile '{name}' 감지 실패(비활성): {e}"),
    });
}

/// SSH config와 Include에서 Host alias를 읽는다. Match exec를 실행할 수 있는 ssh -G는 쓰지 않는다.
/// hostname/user/port는 표시용이며 프로필에 복사하지 않는다.
/// config_exists/readable로 파일 부재·읽기 실패·빈 목록을 구분한다.
/// readable은 최상위 파일만 뜻하며 Include 읽기 실패는 코어에서 경고한다.
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

/// alias만 host에 저장한다. 실제 접속 설정은 접속 시 SSH가 해석한다.
/// 가져오기는 SSH 연결을 시작하지 않는다. 셸 감지는 detect로 별도 요청한다.
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

    #[test]
    fn import_error_codes_are_distinct_and_stable() {
        let unknown = import_error_response(json!(1), ImportError::UnknownAlias("nope".into()));
        let taken = import_error_response(json!(2), ImportError::NameTaken("dup".into()));
        let code = |r: &JsonRpcResponse| r.error.as_ref().expect("error 응답이어야 한다").code;
        assert_eq!(code(&unknown), -32040);
        assert_eq!(code(&taken), -32041);
        assert_ne!(code(&taken), -32602);
        assert!(taken.result.is_none());
    }
}
