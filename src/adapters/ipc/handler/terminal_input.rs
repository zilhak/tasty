//! Global input rules use the same settings update/save path as the settings UI.
use serde_json::{Value, json};

use crate::core::CoreState;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::window_port::IntentOutbox;

pub fn get(engine: &CoreState, id: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(id, json!({ "rules": engine.settings.terminal_input.rules }))
}

pub fn handle_input_rule_update(
    out: &mut IntentOutbox,
    engine: &CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
    method: &str,
) -> JsonRpcResponse {
    let Some(app) = params.get("app").and_then(Value::as_str) else {
        return JsonRpcResponse::invalid_params(id, "'app' must be a string");
    };
    let mut settings = engine.settings.clone();
    let input = &mut settings.terminal_input;
    let result = if method == "settings.remove_input_rule" {
        input.remove_rule(app);
        Ok(true)
    } else {
        let Some(newline) = params.get("shift_enter_newline").and_then(Value::as_bool) else {
            return JsonRpcResponse::invalid_params(id, "'shift_enter_newline' must be a boolean");
        };
        if method == "settings.initialize_input_rule" {
            input.initialize_rule(caller.owner(), app, newline)
        } else {
            input.set_rule(app, newline).map(|()| true)
        }
    };
    match result {
        Err(error) => JsonRpcResponse::invalid_params(id, error),
        Ok(changed) => {
            if changed {
                out.push(
                    crate::core::intent::DomainIntent::UpdateSettings(settings).from_agent_ipc(),
                );
            }
            JsonRpcResponse::success(id, json!({ "changed": changed }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::intent::DomainIntent;
    use crate::intent::Intent;

    #[test]
    fn plugins_can_only_seed_defaults_with_settings_permission() {
        let mut caller = CallerContext::Plugin {
            plugin_id: "com.tasty.claude".into(),
            permissions: Default::default(),
        };
        assert!(
            caller
                .ensure_allowed("settings.initialize_input_rule")
                .is_err()
        );
        if let CallerContext::Plugin { permissions, .. } = &mut caller {
            *permissions =
                std::sync::Arc::new([tasty_plugin_manifest::Permission::UiSettingsPage].into());
        }
        assert!(
            caller
                .ensure_allowed("settings.initialize_input_rule")
                .is_ok()
        );
        for method in [
            "settings.set_input_rule",
            "settings.remove_input_rule",
            "settings.get_input_rules",
        ] {
            assert!(caller.ensure_allowed(method).is_err());
        }
    }

    #[test]
    fn rule_updates_use_settings_intents_and_reject_invalid_values() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let engine = CoreState::new(80, 24, std::sync::Arc::new(|| {})).unwrap();
        let mut out = IntentOutbox::default();
        let response = handle_input_rule_update(
            &mut out,
            &engine,
            &CallerContext::Local,
            json!(1),
            &json!({ "app": "claude", "shift_enter_newline": true }),
            "settings.set_input_rule",
        );
        assert!(response.error.is_none());
        assert!(engine.settings.terminal_input.rules.is_empty());
        let intents = out.into_vec();
        assert_eq!(intents.len(), 1);
        let Intent::Domain(DomainIntent::UpdateSettings(settings)) = &intents[0].body else {
            panic!("expected settings update");
        };
        assert!(
            settings
                .terminal_input
                .shift_enter_newline(Some("claude.exe"))
        );

        for params in [
            json!({}),
            json!({"app":"claude", "shift_enter_newline":"yes"}),
            json!({"app":"", "shift_enter_newline":true}),
        ] {
            let mut out = IntentOutbox::default();
            assert!(
                handle_input_rule_update(
                    &mut out,
                    &engine,
                    &CallerContext::Local,
                    json!(1),
                    &params,
                    "settings.set_input_rule"
                )
                .error
                .is_some()
            );
            assert!(out.is_empty());
        }
    }

    #[test]
    fn plugin_initialization_uses_authenticated_owner_and_does_not_reapply() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut engine = CoreState::new(80, 24, std::sync::Arc::new(|| {})).unwrap();
        let caller = CallerContext::Plugin {
            plugin_id: "com.tasty.claude".into(),
            permissions: Default::default(),
        };
        let params = json!({"app":"claude", "shift_enter_newline":true, "owner":"spoof"});
        let mut out = IntentOutbox::default();
        assert!(
            handle_input_rule_update(
                &mut out,
                &engine,
                &caller,
                json!(1),
                &params,
                "settings.initialize_input_rule"
            )
            .error
            .is_none()
        );
        let intent = out.into_vec().pop().unwrap();
        let Intent::Domain(DomainIntent::UpdateSettings(settings)) = intent.body else {
            panic!("expected settings update");
        };
        engine.settings = settings;
        assert!(
            engine
                .settings
                .terminal_input
                .initialized_defaults
                .contains("com.tasty.claude/claude")
        );
        engine.settings.terminal_input.remove_rule("claude");
        let mut out = IntentOutbox::default();
        let response = handle_input_rule_update(
            &mut out,
            &engine,
            &caller,
            json!(2),
            &params,
            "settings.initialize_input_rule",
        );
        assert_eq!(response.result.unwrap()["changed"], false);
        assert!(out.is_empty());
    }
}
