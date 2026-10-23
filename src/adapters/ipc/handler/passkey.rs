//! 원격 자격증명 관리. 응답에는 name과 kind만 포함하고 경로·내용은 공개하지 않는다.
//! inline 값은 데이터 루트의 passkeys/<name>에 저장하며 Unix에서는 0600을 적용한다(docs/design/systems/memory.md).

use serde_json::{Value, json};

use crate::app::remote_file_commands as files;
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::{PasskeyCommand, Passkeys};

fn passkey_meta(p: &tasty_remote_profiles::Passkey) -> Value {
    json!({ "name": p.name, "kind": p.kind })
}

pub(crate) fn handle_list(id: Value) -> JsonRpcResponse {
    let passkeys = Passkeys::load();
    let arr: Vec<_> = passkeys.passkeys.iter().map(passkey_meta).collect();
    JsonRpcResponse::success(id, json!({ "passkeys": arr }))
}

pub(crate) fn handle_get(id: Value, params: &Value) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let passkeys = Passkeys::load();
    match passkeys.get(name) {
        Some(p) => JsonRpcResponse::success(id, json!({ "passkey": passkey_meta(p) })),
        None => JsonRpcResponse::error(id, -32040, format!("passkey '{name}' not found")),
    }
}

/// path는 사용자 파일을 참조하고 inline은 관리 파일에 저장한다. 값은 응답하지 않는다.
pub(crate) fn handle_add(
    core: &crate::app::services::AppServices,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let Some(kind) = params.get("kind").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(
            id,
            "Missing required 'kind' parameter (path|inline)",
        );
    };
    let Some(value) = params.get("value").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'value' parameter");
    };

    let command = match kind {
        "path" => PasskeyCommand::Path {
            name: name.to_owned(),
            path: value.to_owned(),
        },
        "inline" => PasskeyCommand::Inline {
            name: name.to_owned(),
            secret: value.to_owned(),
        },
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown kind '{other}' (path|inline)"),
            );
        }
    };
    let name = name.to_owned();
    let kind = kind.to_owned();
    files::reply(
        core,
        id,
        files::Request::Passkey(command),
        move |_, id, output| {
            let files::Value::Passkey(saved) = output.value else {
                return JsonRpcResponse::internal_error(id, "unexpected passkey result");
            };
            JsonRpcResponse::success(
                id,
                json!({"saved":true,"name":name,"kind":kind,"replaced":saved.replaced}),
            )
        },
    )
}

/// inline 자격증명이면 관리 파일도 삭제한다.
pub(crate) fn handle_remove(
    core: &crate::app::services::AppServices,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'name' parameter");
    };
    let name = name.to_owned();
    files::reply(
        core,
        id,
        files::Request::Passkey(PasskeyCommand::Remove { name: name.clone() }),
        move |_, id, _| JsonRpcResponse::success(id, json!({"removed":true,"name":name})),
    )
}
