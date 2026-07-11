//! 공유 훅 핸들러 레지스트리의 도메인 타입 (webhook/hook 트리거가 공유).
//!
//! 파일 핸들러(`src/file/handler/types.rs`) 구조를 미러링하되, 트리거 출처를
//! 게이트하는 `HookSource` 를 추가한다. MVP 는 `IpcSequence` / `ShellCommand`
//! 두 action 만 정의한다.
//!
//! ## 불변식 (타입으로 강제)
//! - **데이터/흐름 분리**: [`IpcCall::method`] 는 owner 가 등록 시 고정한 리터럴이며,
//!   치환 엔진(`super::exec`)은 이 타입의 `method` 를 인자로 받지 않는다 — 페이로드가
//!   method 자리에 도달할 코드 경로가 없다.
//! - **셸 웹훅 거부**: [`HookHandlerAction::ShellCommand`] 는 `is_webhook_bindable()`
//!   가 항상 `false` 이고, 레지스트리 등록 시 `source == Hook` 을 강제한다 →
//!   웹훅(외부 HTTP) 바인딩이 구조적으로 불가능.

/// 훅 핸들러의 전역 유일 식별자.
///
/// 형식은 파일 핸들러와 동일: `host/<short>` · `<plugin_id>/<short>` · `user/<short>`.
/// `<short>` 패턴: `[a-z0-9-]{1,32}`.
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

/// short-name 패턴 검증 — `[a-z0-9-]{1,32}` (파일 핸들러와 동일 규약).
pub fn is_valid_hook_handler_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 핸들러의 출처(누가 등록했나).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HookHandlerOwner {
    Host,
    Plugin(String),
    User,
}

impl HookHandlerOwner {
    /// HookHandlerId prefix segment (`host` · `<plugin_id>` · `user`).
    pub fn prefix(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Plugin(id) => id.as_str(),
            Self::User => "user",
        }
    }
}

/// 핸들러가 바인딩 가능한 **트리거 출처** (내부 이벤트 / 외부 HTTP / 둘 다).
///
/// 네트워크 방향(inbound/outbound)이 아니라 트리거 출처로 명명한다 — hook 은
/// 내부 이벤트 + 로컬 동작이라 "outbound" 가 아니기 때문(명세 "네이밍 주의").
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSource {
    /// 내부 이벤트(기존 `tasty-hooks`)에만 바인딩.
    Hook,
    /// 외부 HTTP(웹훅)에만 바인딩.
    Webhook,
    /// 양쪽 모두.
    Any,
}

/// 실제 트리거가 발생한 출처 — 바인딩 게이트 검증의 입력.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    Hook,
    Webhook,
}

impl HookSource {
    /// 이 핸들러를 주어진 트리거 출처에 바인딩할 수 있는가.
    pub fn accepts(self, trigger: TriggerSource) -> bool {
        matches!(
            (self, trigger),
            (HookSource::Any, _)
                | (HookSource::Hook, TriggerSource::Hook)
                | (HookSource::Webhook, TriggerSource::Webhook)
        )
    }
}

/// IpcSequence 의 한 스텝.
///
/// **불변식(데이터/흐름 분리)**: `method` 는 owner 가 등록 시 고정한 리터럴이다.
/// 페이로드는 `params` 의 값 노드(leaf string)에만 `${...}` 로 치환되며,
/// `method` 자리에는 어떤 경로로도 도달하지 못한다(치환 엔진이 `method` 를 아예
/// 인자로 받지 않는다 — `super::exec::substitute_params` 참조).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IpcCall {
    /// owner 가 고정한 IPC 메서드 리터럴. 페이로드에서 오지 못한다.
    pub method: String,
    /// 값 슬롯에만 페이로드가 치환되는 params 템플릿.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// 핸들러가 트리거됐을 때 수행할 동작 (데이터). 실제 실행은 `super::exec` layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookHandlerAction {
    /// **코어 기본, 웹훅용.** owner 가 등록 시 고정한 IPC 호출들의 순차 실행.
    /// tasty 내부 IPC 만 조작(OS 셸 아님). `source: webhook | any`.
    IpcSequence { calls: Vec<IpcCall> },
    /// **기존 `tasty-hooks` legacy** (OS 프로세스). `source: hook` 로만 게이트되어
    /// 웹훅에는 바인딩 불가([`is_webhook_bindable`](HookHandlerAction::is_webhook_bindable)).
    ShellCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl HookHandlerAction {
    /// 이 action 이 외부 HTTP(웹훅) 출처에 바인딩 가능한가.
    ///
    /// 셸(`ShellCommand`)은 **구조적으로 항상 `false`** — 웹훅→셸 경로를 타입
    /// 레벨에서 차단한다(불변식: 셸 웹훅 거부).
    pub fn is_webhook_bindable(&self) -> bool {
        matches!(self, HookHandlerAction::IpcSequence { .. })
    }
}

/// 등록된 훅 핸들러.
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

/// 핸들러를 특정 트리거 출처에 바인딩할 때의 거부 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    /// 핸들러 `source` 가 이 트리거 출처를 허용하지 않음.
    SourceMismatch {
        handler: String,
        declared: HookSource,
        trigger: TriggerSource,
    },
    /// 셸 action 을 웹훅에 바인딩하려 함(구조적으로 불가).
    ShellNotWebhookBindable { handler: String },
    /// 비활성화된 핸들러.
    Disabled { handler: String },
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

/// 바인딩 게이트: 핸들러를 트리거 출처에 연결할 수 있는지 검증한다.
///
/// **불변식(셸 웹훅 거부)**: `Webhook` 트리거는 `is_webhook_bindable()` 가 참인
/// action 만 허용 → `ShellCommand` 는 여기서 거부된다.
pub fn validate_binding(
    handler: &HookHandler,
    trigger: TriggerSource,
) -> Result<(), BindingError> {
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
