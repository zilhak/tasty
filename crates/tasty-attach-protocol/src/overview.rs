//! `workspace.overview` 응답의 형식. 한 번의 상태 읽기로 모은 워크스페이스·서피스 목록과 서버 기능 이름이다.
//! 값이 없는 선택 키(`ws_holder`·`holder`·`granted_seq`)는 쓰지 않고, 읽을 때 없으면 None이다.

use serde::{Deserialize, Serialize};

/// `workspace.overview`의 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceOverview {
    /// 서버의 워크스페이스.
    pub workspaces: Vec<OverviewWorkspace>,
    /// 서버 기능 이름 목록.
    pub capabilities: Vec<String>,
}

/// 워크스페이스 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewWorkspace {
    /// 서버의 workspace ID.
    pub id: u32,
    /// 워크스페이스 이름.
    pub name: String,
    /// 워크스페이스 부제.
    pub subtitle: String,
    /// 활동 중인 서피스 수.
    pub busy_count: usize,
    /// 다른 서버를 보여 주는 mirror 워크스페이스인지.
    pub mirror: bool,
    /// 워크스페이스를 점유한 attach client. 점유가 없으면 None이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ws_holder: Option<u32>,
    /// 워크스페이스의 서피스.
    pub surfaces: Vec<OverviewSurface>,
}

/// 서피스 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewSurface {
    /// 서버의 surface ID.
    pub id: u32,
    /// 서피스 종류(`terminal` 또는 등록된 plugin kind).
    pub kind: String,
    /// 표시 이름.
    pub title: String,
    /// 활동 중인지.
    pub busy: bool,
    /// 서피스를 점유한 attach client. 점유가 없으면 None이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<u32>,
    /// 그 점유의 부여 순번. 점유가 없으면 None이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_seq: Option<u64>,
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
