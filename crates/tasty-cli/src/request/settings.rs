//! `tasty settings` CLI → JsonRpcRequest mapping.

use crate::commands::SettingsCommands;

pub(super) fn settings_command_to_method_params(
    command: &SettingsCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        SettingsCommands::GetInputRules => ("settings.get_input_rules", serde_json::json!({})),
        SettingsCommands::SetInputRule {
            app,
            shift_enter_newline,
        } => (
            "settings.set_input_rule",
            serde_json::json!({ "app": app, "shift_enter_newline": shift_enter_newline }),
        ),
        SettingsCommands::InitializeInputRule {
            app,
            shift_enter_newline,
        } => (
            "settings.initialize_input_rule",
            serde_json::json!({ "app": app, "shift_enter_newline": shift_enter_newline }),
        ),
        SettingsCommands::RemoveInputRule { app } => (
            "settings.remove_input_rule",
            serde_json::json!({ "app": app }),
        ),
        SettingsCommands::GetRemoteTransfer => {
            ("settings.get_remote_transfer", serde_json::json!({}))
        }
        SettingsCommands::SetRemoteTransfer { dir, max_mb } => {
            // 지정된 필드만 실어 보낸다(핸들러가 부분 patch 로 현재 설정 위에 덮음).
            let mut params = serde_json::Map::new();
            if let Some(dir) = dir {
                params.insert("dir".to_string(), serde_json::json!(dir));
            }
            if let Some(max_mb) = max_mb {
                params.insert("max_mb".to_string(), serde_json::json!(max_mb));
            }
            (
                "settings.set_remote_transfer",
                serde_json::Value::Object(params),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn input_rule_cli_keeps_false_as_a_value() {
        let cli = Cli::try_parse_from([
            "tasty",
            "settings",
            "set-input-rule",
            "--app",
            "claude",
            "--shift-enter-newline",
            "false",
        ])
        .unwrap();
        let Commands::Settings { command } = cli.command.unwrap() else {
            panic!("settings command");
        };
        let (method, params) = super::settings_command_to_method_params(&command);
        assert_eq!(method, "settings.set_input_rule");
        assert_eq!(params["app"], "claude");
        assert_eq!(params["shift_enter_newline"], false);
    }
}
