//! 핸들러 시스템 (`file_handler`)의 도메인 타입.
//!
//! `file_format::DetectorId` 만 import 한다 (단방향). evaluator / rule kind 는 모름.

use crate::file_format::DetectorId;

/// 핸들러의 전역 유일 식별자.
///
/// 형식:
/// - 호스트: `host/<short-name>` (예: `host/markdown-viewer`)
/// - Plugin: `<plugin_id>/<short-name>` (예: `com.tasty.image/viewer`)
/// - User:   `user/<short-name>` (예: `user/my-pdf-opener`)
///
/// `<short-name>` 패턴: `[a-z0-9-]{1,32}`. slash 추가 금지.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HandlerId(pub String);

impl HandlerId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HandlerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// short-name 패턴 검증.
pub fn is_valid_handler_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 핸들러의 출처.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HandlerOwner {
    Host,
    Plugin(String),
    User,
}

impl HandlerOwner {
    /// HandlerId prefix segment 추출 (e.g. `host`, `<plugin_id>`, `user`).
    pub fn prefix(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Plugin(id) => id.as_str(),
            Self::User => "user",
        }
    }
}

/// 핸들러가 트리거됐을 때 수행할 동작 명세 (데이터). 실제 실행은 호스트 본체 layer.
#[derive(Debug, Clone)]
pub enum HandlerAction {
    /// 포커스 pane 에 surface kind 탭 추가.
    OpenSurface {
        surface_kind: String,
        param_key: String,
    },
    /// plugin IPC 메서드 호출. method 는 owner_plugin_id 의 namespace prefix.
    Ipc {
        method: String,
        owner_plugin_id: String,
    },
    /// OS 기본 file opener 위임 (Finder / Explorer / xdg-open).
    System,
}

/// 등록된 핸들러.
#[derive(Debug, Clone)]
pub struct FileHandler {
    pub id: HandlerId,
    pub detector: DetectorId,
    pub priority: i32,
    pub owner: HandlerOwner,
    pub action: HandlerAction,
    pub display_name_i18n_key: Option<String>,
    pub disabled: bool,
}
