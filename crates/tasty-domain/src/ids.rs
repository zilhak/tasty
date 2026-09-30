//! 저널 전용 typed ID와 ID 공급자.
//!
//! 값 공간은 runtime의 기존 ID와 별개다. 공급자는 한 번 준 값을 다시 주지 않으며 빈 구간은
//! 허용한다. 영속 예약은 저장소 쪽 공급자가 맡고, 이 크레이트는 공급 계약만 정한다.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// ID를 새로 받아 오는 곳. `space`마다 독립된 값 공간이다.
pub trait IdSupplier {
    fn next_id(&mut self, space: &'static str) -> u64;
}

/// 메모리에서만 증가하는 공급자. 재시작하면 처음부터 다시 세므로 시험용이다.
#[derive(Debug, Clone, Default)]
pub struct MemoryIdSupplier {
    last: BTreeMap<&'static str, u64>,
}

impl MemoryIdSupplier {
    /// `space`가 다음에 줄 값이 `last + 1`이 되도록 시작점을 정한다.
    pub fn starting_after(mut self, space: &'static str, last: u64) -> Self {
        self.last.insert(space, last);
        self
    }
}

impl IdSupplier for MemoryIdSupplier {
    fn next_id(&mut self, space: &'static str) -> u64 {
        let slot = self.last.entry(space).or_insert(0);
        *slot += 1;
        *slot
    }
}

macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident, $space:literal, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl $name {
            /// 공급자에서 이 종류의 값 공간을 쓴다.
            pub const SPACE: &'static str = $space;

            pub fn allocate(ids: &mut dyn IdSupplier) -> Self {
                Self(ids.next_id(Self::SPACE))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

typed_id!(
    /// workspace 카테고리.
    CategoryId,
    "category",
    "category:"
);
typed_id!(WorkspaceId, "workspace", "workspace:");
typed_id!(PaneId, "pane", "pane:");
typed_id!(TabId, "tab", "tab:");
typed_id!(SurfaceId, "surface", "surface:");
