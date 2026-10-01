//! 저널 위치와 ID 공급 계약.
//!
//! workspace·category·pane·tab·surface ID는 `tasty-model`의 타입을 그대로 쓴다. 공급자는 한 번 준
//! 값을 다시 주지 않으며 빈 구간은 허용한다. 영속 예약은 실행 계층이 판단 전에 완료한다.

use serde::{Deserialize, Serialize};

/// 구조 stream 안의 확정 이벤트 순번. 첫 이벤트가 1이다.
pub type Revision = u64;

/// journal 안의 확정 batch 번호. commit 순서대로 증가한다.
pub type BatchId = u64;

/// 새 ID를 받을 값 공간.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IdKind {
    Category,
    Workspace,
    Pane,
    Tab,
    Surface,
}

impl IdKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Workspace => "workspace",
            Self::Pane => "pane",
            Self::Tab => "tab",
            Self::Surface => "surface",
        }
    }
}
