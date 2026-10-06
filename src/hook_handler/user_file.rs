//! 사용자 설정 파일(hook-handlers.toml)에서 IpcSequence 호출의 params를 표현하는 방법.
//! TOML에는 null이 없다. params 자체가 null이면 키를 빼고, 읽을 때 `IpcCall::params`의 serde
//! 기본값이 null을 다시 채운다. 안쪽에 null이 있는 params는 JSON 문자열 `params_json`으로 쓴다.
//! null이 없는 params는 TOML 표로 쓰므로 이 형식 이전에 만든 파일도 그대로 읽힌다.

use super::types::HookHandlerAction;

const PARAMS: &str = "params";
const PARAMS_JSON: &str = "params_json";

/// 사용자 action을 TOML 값으로 바꾼다.
pub(super) fn action_toml(action: &HookHandlerAction) -> Result<toml::Value, String> {
    let mut json = serde_json::to_value(action).map_err(|e| e.to_string())?;
    for call in calls_mut(&mut json) {
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

/// 읽은 사용자 설정 문서를 JSON으로 바꾸고 각 호출의 `params_json`을 `params`로 되돌린다.
pub(super) fn restore_params(doc: toml::Value) -> Result<serde_json::Value, String> {
    let mut json = serde_json::to_value(doc).map_err(|e| e.to_string())?;
    let handlers = json
        .get_mut("handler")
        .and_then(serde_json::Value::as_array_mut);
    for (index, handler) in handlers.into_iter().flatten().enumerate() {
        let Some(action) = handler.get_mut("action") else {
            continue;
        };
        for call in calls_mut(action) {
            let Some(text) = call.remove(PARAMS_JSON) else {
                continue;
            };
            if call.contains_key(PARAMS) {
                return Err(format!(
                    "handler[{index}]: a call has both {PARAMS} and {PARAMS_JSON}"
                ));
            }
            let text = text
                .as_str()
                .ok_or_else(|| format!("handler[{index}]: {PARAMS_JSON} must be a string"))?;
            let params = serde_json::from_str(text)
                .map_err(|e| format!("handler[{index}]: {PARAMS_JSON} is not JSON: {e}"))?;
            call.insert(PARAMS.into(), params);
        }
    }
    Ok(json)
}

fn calls_mut(
    action: &mut serde_json::Value,
) -> impl Iterator<Item = &mut serde_json::Map<String, serde_json::Value>> {
    action
        .get_mut("calls")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object_mut)
}

fn contains_null(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(contains_null),
        serde_json::Value::Object(map) => map.values().any(contains_null),
        _ => false,
    }
}
