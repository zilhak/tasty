//! host·사용자 선언은 IPC 시퀀스와 셸 명령을, plugin 선언은 IPC 시퀀스만 받는다.
//! 셸 명령의 source=hook 제한은 별도 검증 함수와 registry 병합에서 확인한다.

use std::fmt;

use serde::Deserialize;

use super::types::{HookHandlerAction, HookSource, IpcCall, is_valid_hook_handler_short_name};

#[derive(Debug, Clone, Deserialize)]
pub struct HookHandlerDecl<A> {
    /// owner 접두사와 합쳐 전역 ID를 만든다.
    pub id: String,
    pub source: HookSource,
    pub priority: i32,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    pub action: A,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostHookHandlerActionDecl {
    IpcSequence {
        calls: Vec<IpcCall>,
    },
    ShellCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

/// plugin 선언에는 셸 명령 variant가 없어 역직렬화에서 거부된다.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginHookHandlerActionDecl {
    IpcSequence { calls: Vec<IpcCall> },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserHookHandlerActionDecl {
    IpcSequence {
        calls: Vec<IpcCall>,
    },
    ShellCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl From<HostHookHandlerActionDecl> for HookHandlerAction {
    fn from(d: HostHookHandlerActionDecl) -> Self {
        match d {
            HostHookHandlerActionDecl::IpcSequence { calls } => {
                HookHandlerAction::IpcSequence { calls }
            }
            HostHookHandlerActionDecl::ShellCommand { command, args } => {
                HookHandlerAction::ShellCommand { command, args }
            }
        }
    }
}

impl From<PluginHookHandlerActionDecl> for HookHandlerAction {
    fn from(d: PluginHookHandlerActionDecl) -> Self {
        match d {
            PluginHookHandlerActionDecl::IpcSequence { calls } => {
                HookHandlerAction::IpcSequence { calls }
            }
        }
    }
}

impl From<UserHookHandlerActionDecl> for HookHandlerAction {
    fn from(d: UserHookHandlerActionDecl) -> Self {
        match d {
            UserHookHandlerActionDecl::IpcSequence { calls } => {
                HookHandlerAction::IpcSequence { calls }
            }
            UserHookHandlerActionDecl::ShellCommand { command, args } => {
                HookHandlerAction::ShellCommand { command, args }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookHandlerDeclError {
    InvalidShortName(String),
    ShellMustBeHookSource { handler: String },
}

impl fmt::Display for HookHandlerDeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShortName(s) => write!(
                f,
                "invalid hook handler short-name '{s}' (must match [a-z0-9-]{{1,32}})"
            ),
            Self::ShellMustBeHookSource { handler } => write!(
                f,
                "hook handler '{handler}' is a shell command and must declare source = hook"
            ),
        }
    }
}

impl std::error::Error for HookHandlerDeclError {}

pub fn validate_plugin_hook_handler_decl(
    decl: &HookHandlerDecl<PluginHookHandlerActionDecl>,
) -> Result<(), HookHandlerDeclError> {
    if !is_valid_hook_handler_short_name(&decl.id) {
        return Err(HookHandlerDeclError::InvalidShortName(decl.id.clone()));
    }
    Ok(())
}

pub fn validate_host_hook_handler_decl(
    decl: &HookHandlerDecl<HostHookHandlerActionDecl>,
) -> Result<(), HookHandlerDeclError> {
    if !is_valid_hook_handler_short_name(&decl.id) {
        return Err(HookHandlerDeclError::InvalidShortName(decl.id.clone()));
    }
    if matches!(decl.action, HostHookHandlerActionDecl::ShellCommand { .. })
        && decl.source != HookSource::Hook
    {
        return Err(HookHandlerDeclError::ShellMustBeHookSource {
            handler: decl.id.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug)]
    struct PluginWrap {
        #[serde(rename = "handler")]
        handlers: Vec<HookHandlerDecl<PluginHookHandlerActionDecl>>,
    }

    fn parse_plugin(s: &str) -> Result<PluginWrap, toml::de::Error> {
        toml::from_str(s)
    }

    #[derive(Deserialize, Debug)]
    struct HostWrap {
        #[serde(rename = "handler")]
        handlers: Vec<HookHandlerDecl<HostHookHandlerActionDecl>>,
    }

    fn parse_host(s: &str) -> Result<HostWrap, toml::de::Error> {
        toml::from_str(s)
    }

    #[test]
    fn host_can_use_shell_command() {
        let t = r#"
            [[handler]]
            id = "notify-shell"
            source = "hook"
            priority = 50
            [handler.action]
            kind = "shell_command"
            command = "echo"
            args = ["hi"]
        "#;
        let h = parse_host(t).expect("host parse");
        assert_eq!(h.handlers.len(), 1);
        assert!(matches!(
            h.handlers[0].action,
            HostHookHandlerActionDecl::ShellCommand { .. }
        ));
    }

    #[test]
    fn plugin_shell_command_rejected() {
        let t = r#"
            [[handler]]
            id = "x"
            source = "webhook"
            priority = 1
            [handler.action]
            kind = "shell_command"
            command = "echo"
        "#;
        let err = parse_plugin(t).expect_err("plugin must reject shell_command");
        let msg = format!("{err}").to_lowercase();
        assert!(msg.contains("shell") || msg.contains("unknown variant"));
    }

    #[test]
    fn plugin_ipc_sequence_parses() {
        let t = r#"
            [[handler]]
            id = "notify"
            source = "webhook"
            priority = 100
            [handler.action]
            kind = "ipc_sequence"
            calls = [{ method = "notification.create", params = { body = "hi" } }]
        "#;
        let h = parse_plugin(t).expect("parse");
        assert_eq!(h.handlers[0].id, "notify");
        match &h.handlers[0].action {
            PluginHookHandlerActionDecl::IpcSequence { calls } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].method, "notification.create");
            }
        }
    }

    #[test]
    fn host_shell_with_non_hook_source_rejected() {
        let decl = HookHandlerDecl::<HostHookHandlerActionDecl> {
            id: "sh".into(),
            source: HookSource::Any,
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: HostHookHandlerActionDecl::ShellCommand {
                command: "echo".into(),
                args: vec![],
            },
        };
        assert!(matches!(
            validate_host_hook_handler_decl(&decl),
            Err(HookHandlerDeclError::ShellMustBeHookSource { .. })
        ));
    }

    #[test]
    fn host_shell_with_hook_source_ok() {
        let decl = HookHandlerDecl::<HostHookHandlerActionDecl> {
            id: "sh".into(),
            source: HookSource::Hook,
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: HostHookHandlerActionDecl::ShellCommand {
                command: "echo".into(),
                args: vec![],
            },
        };
        assert!(validate_host_hook_handler_decl(&decl).is_ok());
    }

    #[test]
    fn validate_rejects_bad_short_name() {
        let decl = HookHandlerDecl::<PluginHookHandlerActionDecl> {
            id: "Bad/Name".into(),
            source: HookSource::Webhook,
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHookHandlerActionDecl::IpcSequence { calls: vec![] },
        };
        assert!(matches!(
            validate_plugin_hook_handler_decl(&decl),
            Err(HookHandlerDeclError::InvalidShortName(_))
        ));
    }
}
