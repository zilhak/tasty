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

use crate::core::engine_access::{EngineMut, EngineRef};
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

/// 엔진 수명 원본. 창 연결은 App registry에 있고 실행 자원은 이 객체와 함께 산다.
/// observer/hook/task를 Terminal보다 먼저 정리한다. TaskScope drop은 task 취소가 아니다.
pub(crate) struct EngineSession {
    pub(crate) id: EngineId,
    pub(crate) core_state: CoreState,
    pub(crate) hooks: crate::hook_runtime::HookRuntimeState,
    pub(crate) task_scope: crate::core::task_service::TaskScope,
    pub(crate) observer_router: crate::output_observer::ObserverRouter,
    pub(crate) runtime: crate::core::engine_runtime::EngineRuntime,
    /// 실행 자원의 Drop까지 격리 홈이 살아 있어야 한다.
    #[cfg(test)]
    _isolated_home: Option<crate::test_support::IsolatedHome>,
}

impl EngineSession {
    pub(crate) fn borrow_mut(&mut self) -> EngineMut<'_> {
        EngineMut {
            core: &mut self.core_state,
            runtime: &mut self.runtime,
            hooks: &mut self.hooks,
            task_scope: &mut self.task_scope,
            observer_router: &mut self.observer_router,
        }
    }

    pub(crate) fn as_ref(&self) -> EngineRef<'_> {
        EngineRef {
            core: &self.core_state,
            runtime: &self.runtime,
            hooks: &self.hooks,
            task_scope: &self.task_scope,
            observer_router: &self.observer_router,
        }
    }
}

mod bootstrap;
