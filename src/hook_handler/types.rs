//! 훅·웹훅 핸들러 타입. payload는 params 값에만 치환한다.
//! ShellCommand의 웹훅 거부는 바인딩 검증과 등록부의 source 검사로 적용한다.

/// host/short, plugin_id/short 또는 user/short 형식의 ID. 생성자 자체가 문법을 검증하지는 않는다.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct HookHandlerId(pub String);

impl HookHandlerId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HookHandlerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn is_valid_hook_handler_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HookHandlerOwner {
    Host,
    Plugin(String),
    User,
}

impl HookHandlerOwner {
    pub fn prefix(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Plugin(id) => id.as_str(),
            Self::User => "user",
        }
    }
}

/// 네트워크 방향이 아닌 트리거 출처. 내부 훅·외부 HTTP·양쪽 중 하나다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSource {
    Hook,
    Webhook,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    Hook,
    Webhook,
}

impl HookSource {
    pub fn accepts(self, trigger: TriggerSource) -> bool {
        matches!(
            (self, trigger),
            (HookSource::Any, _)
                | (HookSource::Hook, TriggerSource::Hook)
                | (HookSource::Webhook, TriggerSource::Webhook)
        )
    }
}

/// 등록된 method와 치환할 params. 실행기는 method를 payload로 바꾸지 않는다.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IpcCall {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookHandlerAction {
    /// 등록된 IPC 요청의 순서대로 실행한다. hook과 webhook에 사용할 수 있다.
    IpcSequence { calls: Vec<IpcCall> },
    /// source=hook에서만 허용하는 OS 명령.
    ShellCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl HookHandlerAction {
    /// 웹훅에 연결할 수 있는 action인지 반환한다. 호출자가 이 검사를 적용해야 한다.
    pub fn is_webhook_bindable(&self) -> bool {
        matches!(self, HookHandlerAction::IpcSequence { .. })
    }
}

#[derive(Debug, Clone)]
pub struct HookHandler {
    pub id: HookHandlerId,
    pub source: HookSource,
    pub priority: i32,
    pub owner: HookHandlerOwner,
    pub action: HookHandlerAction,
    pub display_name_i18n_key: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    SourceMismatch {
        handler: String,
        declared: HookSource,
        trigger: TriggerSource,
    },
    ShellNotWebhookBindable {
        handler: String,
    },
    Disabled {
        handler: String,
    },
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceMismatch {
                handler,
                declared,
                trigger,
            } => write!(
                f,
                "handler '{handler}' declares source {declared:?}, cannot bind to {trigger:?} trigger"
            ),
            Self::ShellNotWebhookBindable { handler } => write!(
                f,
                "handler '{handler}' is a shell command and cannot bind to a webhook (webhooks operate tasty IPC only)"
            ),
            Self::Disabled { handler } => write!(f, "handler '{handler}' is disabled"),
        }
    }
}

pub fn validate_binding(handler: &HookHandler, trigger: TriggerSource) -> Result<(), BindingError> {
    if handler.disabled {
        return Err(BindingError::Disabled {
            handler: handler.id.0.clone(),
        });
    }
    if !handler.source.accepts(trigger) {
        return Err(BindingError::SourceMismatch {
            handler: handler.id.0.clone(),
            declared: handler.source,
            trigger,
        });
    }
    if trigger == TriggerSource::Webhook && !handler.action.is_webhook_bindable() {
        return Err(BindingError::ShellNotWebhookBindable {
            handler: handler.id.0.clone(),
        });
    }
    Ok(())
}
