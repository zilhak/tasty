//! 한 engine의 수명 단위. 창·parked·임시 어느 자리에 있든 같은 id로 식별한다.

// 상위 모듈의 dead_code 허용은 시험 전용 journal 때문이다. 이 모듈은 제품 경로라 검사를 되살린다.
#![warn(dead_code)]
#![cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "headless는 창·parked가 없어 engine을 지역 변수로 들고 id를 쓰지 않는다"
    )
)]

use std::sync::atomic::{AtomicU32, Ordering};

use crate::core::CoreState;

/// 프로세스 안에서만 쓰는 engine 번호. 창 ID·레이아웃 슬롯·IPC ID와 별개이며 저장하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EngineId(u32);

impl EngineId {
    /// 발급 순서대로 증가한다. 한 프로세스에서 같은 값을 두 번 주지 않는다.
    fn issue() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// engine 하나와 그 id. 창에 놓이면 MainView가 둘을 나눠 보관한다.
pub(crate) struct EngineSession {
    pub(crate) id: EngineId,
    pub(crate) core_state: CoreState,
}

impl EngineSession {
    /// 새 engine에 id를 붙인다. 이미 id가 있는 engine을 다시 감쌀 때는 필드로 만든다.
    pub(crate) fn new(core_state: CoreState) -> Self {
        Self {
            id: EngineId::issue(),
            core_state,
        }
    }
}
