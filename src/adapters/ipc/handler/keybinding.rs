//! 창 없이 단축키 설정의 한 동작 바인딩을 조회한다. GUI와 헤드리스가 같은 함수를 쓴다.

use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;

/// webview 페이지는 host 키 설정을 받지 않으므로 페이지가 직접 처리하는 동작의 조합을
/// 이 조회로 읽어 문서에 넣는다. 저장 형식의 바인딩 문자열을 그대로 돌려준다.
pub fn handle_query(
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let Some(action) = params.get("action").and_then(Value::as_str) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'action' parameter");
    };
    match engine.settings.keybindings.get_bindings(action) {
        Some(bindings) => {
            JsonRpcResponse::success(id, json!({ "action": action, "bindings": bindings }))
        }
        None => JsonRpcResponse::error(id, -32602, format!("unknown keybinding action '{action}'")),
    }
}

/// 바인딩이 달라진 동작 ID. `command.host_shortcut_changed` 의 payload 다.
/// 이벤트는 plugin을 싣는 GUI만 발행한다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn changed_actions(
    prev: &crate::settings::KeybindingSettings,
    next: &crate::settings::KeybindingSettings,
) -> Vec<&'static str> {
    crate::settings::KeybindingSettings::binding_fields()
        .map(|(id, _)| *id)
        .filter(|id| prev.get_bindings(id) != next.get_bindings(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_actions_lists_only_the_rebound_field() {
        let prev = crate::settings::KeybindingSettings::default();
        let mut next = prev.clone();
        next.find = vec!["ctrl+shift+g".to_string()];
        assert_eq!(changed_actions(&prev, &next), vec!["find"]);
        assert!(changed_actions(&prev, &prev).is_empty());
    }
}
