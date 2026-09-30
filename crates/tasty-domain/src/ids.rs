//! 저널 위치와 ID 공급 계약.
//!
//! workspace·category·pane·tab·surface ID는 `tasty-model`의 타입을 그대로 쓴다. 공급자는 한 번 준
//! 값을 다시 주지 않으며 빈 구간은 허용한다. 영속 예약은 저장소 쪽 공급자가 맡는다.

use std::collections::BTreeMap;

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

/// ID를 새로 받아 오는 곳. decide는 새 ID를 여기서만 받는다.
pub trait IdSupplier {
    fn next_id(&mut self, kind: IdKind) -> u32;
}

/// 메모리에서만 증가하는 공급자. 재시작하면 처음부터 다시 세므로 시험용이다.
#[derive(Debug, Clone, Default)]
pub struct MemoryIdSupplier {
    last: BTreeMap<IdKind, u32>,
}

impl MemoryIdSupplier {
    /// `kind`가 다음에 줄 값이 `last + 1`이 되도록 시작점을 정한다.
    pub fn starting_after(mut self, kind: IdKind, last: u32) -> Self {
        self.last.insert(kind, last);
        self
    }
}

impl IdSupplier for MemoryIdSupplier {
    fn next_id(&mut self, kind: IdKind) -> u32 {
        let slot = self.last.entry(kind).or_insert(0);
        *slot += 1;
        *slot
    }
}
