//! 사용자 설정 파일(hook-handlers.toml)에서 IpcSequence 호출의 params를 표현하는 방법.
//! TOML에는 null이 없다. params 자체가 null이면 키를 빼고, 읽을 때 null로 채운다.
//! 안쪽에 null이 있는 params는 JSON 문자열 `params_json`으로 쓴다. null 키를 지우지 않는 것은
//! 받는 IPC 메서드가 키의 존재 여부로 동작을 가를 수 있어서다.
//! null이 없는 params는 TOML 표로 쓰므로 이 형식 이전에 만든 파일도 그대로 읽힌다.
//! 읽기는 TOML에서 바로 역직렬화해 타입 오류에도 toml이 줄·열 위치를 붙인다. TOML을 JSON 값으로
//! 바꾼 뒤 역직렬화하면 그 위치와 원문 발췌가 경고 로그에서 빠진다.

use serde::Deserialize;

use super::types::{HookHandlerAction, IpcCall};

const PARAMS: &str = "params";
const PARAMS_JSON: &str = "params_json";

/// 파일에 적힌 사용자 action. 호출 형식만 IPC 선언(`UserHookHandlerActionDecl`)과 다르다.
/// action 종류나 필드를 추가하면 두 타입을 함께 고친다.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum UserFileActionDecl {
    IpcSequence {
        calls: Vec<UserFileIpcCall>,
    },
    ShellCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl From<UserFileActionDecl> for HookHandlerAction {
    fn from(d: UserFileActionDecl) -> Self {
        match d {
            UserFileActionDecl::IpcSequence { calls } => HookHandlerAction::IpcSequence {
                calls: calls.into_iter().map(|c| c.0).collect(),
            },
            UserFileActionDecl::ShellCommand { command, args } => {
                HookHandlerAction::ShellCommand { command, args }
            }
        }
    }
}

/// `params` 표와 `params_json` 문자열 중 하나로 적힌 호출.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawUserFileIpcCall")]
pub(super) struct UserFileIpcCall(IpcCall);

#[derive(Deserialize)]
struct RawUserFileIpcCall {
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    params_json: Option<String>,
}

impl TryFrom<RawUserFileIpcCall> for UserFileIpcCall {
    type Error = String;

    fn try_from(raw: RawUserFileIpcCall) -> Result<Self, String> {
        let params = match (raw.params, raw.params_json) {
            (Some(_), Some(_)) => {
                return Err(format!("a call has both {PARAMS} and {PARAMS_JSON}"));
            }
            (None, Some(text)) => serde_json::from_str(&text)
                .map_err(|e| format!("{PARAMS_JSON} is not JSON: {e}"))?,
            (params, None) => params.unwrap_or_default(),
        };
        Ok(Self(IpcCall {
            method: raw.method,
            params,
        }))
    }
}

/// 사용자 action을 TOML 값으로 바꾼다.
pub(super) fn action_toml(action: &HookHandlerAction) -> Result<toml::Value, String> {
    let mut json = serde_json::to_value(action).map_err(|e| e.to_string())?;
    let calls = json
        .get_mut("calls")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object_mut);
    for call in calls {
        let Some(params) = call.get(PARAMS) else {
            continue;
        };
        if params.is_null() {
            call.remove(PARAMS);
        } else if contains_null(params) {
            let text = serde_json::to_string(params).map_err(|e| e.to_string())?;
            call.remove(PARAMS);
            call.insert(PARAMS_JSON.into(), serde_json::Value::String(text));
        }
    }
    toml::Value::try_from(json).map_err(|e| e.to_string())
}

fn contains_null(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(contains_null),
        serde_json::Value::Object(map) => map.values().any(contains_null),
        _ => false,
    }
}
