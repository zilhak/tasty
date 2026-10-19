//! `plugin install` 이 미인증이라 거부됐을 때의 경고. 호스트 오류의 `data.unverified` 로 원인을
//! 알고, 위험과 명시적으로 고르는 방법을 현재 언어로 보인다.

use tasty_ipc::client::JsonRpcCallError;

use crate::Commands;
use crate::commands::PluginCommands;

/// 미인증 거부면 경고 문단을, 아니면 `None` 을 돌려준다.
pub(crate) fn unverified_install_warning(command: &Commands, e: &anyhow::Error) -> Option<String> {
    let Commands::Plugin {
        command: PluginCommands::Install { path, .. },
    } = command
    else {
        return None;
    };
    let data = e.downcast_ref::<JsonRpcCallError>()?.data.as_ref()?;
    let cause_key = match data.get("unverified")?.as_str()? {
        "unknown-key" => "cli.plugin.unverified_cause_unknown_key",
        "missing-pubkey" => "cli.plugin.unverified_cause_missing_pubkey",
        "unsigned" => "cli.plugin.unverified_cause_unsigned",
        _ => "cli.plugin.unverified_cause_signature_error",
    };
    let plugin_id = data
        .get("plugin_id")
        .and_then(|v| v.as_str())
        .unwrap_or(path);
    Some(tasty_i18n::t_args(
        "cli.plugin.unverified_refused",
        &[plugin_id, tasty_i18n::t(cause_key), path],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(data: serde_json::Value) -> anyhow::Error {
        JsonRpcCallError {
            code: -32000,
            message: "plugin 'com.example.x' is unverified".into(),
            data: Some(data),
        }
        .into()
    }

    fn install() -> Commands {
        Commands::Plugin {
            command: PluginCommands::Install {
                path: "./x".into(),
                allow_unverified: false,
            },
        }
    }

    #[test]
    fn an_unverified_refusal_names_the_cause_and_the_flag() {
        tasty_i18n::init("en");
        let warning = unverified_install_warning(
            &install(),
            &refusal(serde_json::json!({"unverified": "unsigned", "plugin_id": "com.example.x"})),
        )
        .expect("warning");
        assert!(warning.contains("com.example.x"), "{warning}");
        assert!(warning.contains(tasty_i18n::t("cli.plugin.unverified_cause_unsigned")));
        assert!(
            warning.contains("tasty plugin install --allow-unverified ./x"),
            "{warning}"
        );
    }

    #[test]
    fn other_failures_get_no_warning() {
        assert!(unverified_install_warning(&install(), &refusal(serde_json::json!({}))).is_none());
        assert!(unverified_install_warning(&install(), &anyhow::anyhow!("copy failed")).is_none());
    }
}
