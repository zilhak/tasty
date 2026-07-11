//! Hook handler TOML/manifest schema. Actor 별 action variant 차이를 schema 에서
//! 강제한다 (파일 핸들러 `src/file/handler/config.rs` 미러).
//!
//! - `HostHookHandlerActionDecl`: `IpcSequence` / `ShellCommand`
//! - `PluginHookHandlerActionDecl`: `IpcSequence` (**ShellCommand 없음** — manifest reject)
//! - `UserHookHandlerActionDecl`: `IpcSequence` / `ShellCommand`
//!
//! 셸(`ShellCommand`)은 OS 프로세스를 띄우는 위험 action 이라 파일 핸들러의 `System`
//! 과 같은 지위 — host/user 만 선언할 수 있고 plugin 은 타입 레벨에서 배제한다.
//! 추가로 `ShellCommand` 는 `source = hook` 만 허용하며, 이 불변식은 레지스트리
//! finalize 단계에서 구조적으로 강제된다(`registry.rs`).

use std::fmt;

use serde::Deserialize;

use super::types::{HookHandlerAction, HookSource, IpcCall, is_valid_hook_handler_short_name};

/// Hook handler 정의의 actor-agnostic 표면. 파일 핸들러 `HandlerDecl<A>` 미러이되
/// `detector` 대신 트리거 출처 게이트 `source` 를 갖는다.
#[derive(Debug, Clone, Deserialize)]
pub struct HookHandlerDecl<A> {
    /// short-name. 전역 id 로 합쳐질 때 `<owner_prefix>/<short-name>` 이 된다.
    pub id: String,
    /// 이 핸들러가 바인딩 가능한 트리거 출처(hook / webhook / any).
    pub source: HookSource,
    pub priority: i32,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    pub action: A,
}

/// Host default 가 사용 가능한 action set (IpcSequence + ShellCommand).
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

/// Plugin manifest 가 사용 가능한 action set. **`ShellCommand` variant 없음** —
/// manifest 에 `kind = "shell_command"` 적으면 serde unknown variant 로 reject.
/// (파일 핸들러 plugin 이 `System` 을 못 쓰는 것과 동일한 지위.)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginHookHandlerActionDecl {
    IpcSequence { calls: Vec<IpcCall> },
}

/// User config 가 사용 가능한 action set (IpcSequence + ShellCommand).
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

/// Hook handler decl schema 검증 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookHandlerDeclError {
    InvalidShortName(String),
    /// 셸 action 을 `source != hook` 으로 선언(불변식 위반).
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

/// Plugin decl 단독 schema 검증. plugin 은 `ShellCommand` 를 타입상 못 쓰므로 셸
/// 게이트는 불필요하고 short-name 만 확인한다.
pub fn validate_plugin_hook_handler_decl(
    decl: &HookHandlerDecl<PluginHookHandlerActionDecl>,
) -> Result<(), HookHandlerDeclError> {
    if !is_valid_hook_handler_short_name(&decl.id) {
        return Err(HookHandlerDeclError::InvalidShortName(decl.id.clone()));
    }
    Ok(())
}

/// Host decl 검증. short-name + 셸 불변식(`ShellCommand` ⇒ `source == hook`).
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
