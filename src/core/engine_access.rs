//! 실행 경로가 필요한 원본만 빌린다. EngineSession의 수명·ID·View 관계는 노출하지 않는다.
//! 구조만 읽거나 바꾸는 함수는 계속 CoreState를 받는다. 이 대여는 effect 분리를 대신하지 않는다.
use super::{CoreState, engine_runtime::EngineRuntime, task_service::TaskScope};
use crate::hook_runtime::HookRuntimeState;
use crate::output_observer::ObserverRouter;
use std::ops::{Deref, DerefMut};

pub(crate) struct EngineMut<'a> {
    pub(crate) core: &'a mut CoreState,
    pub(crate) runtime: &'a mut EngineRuntime,
    pub(crate) hooks: &'a mut HookRuntimeState,
    pub(crate) task_scope: &'a mut TaskScope,
    pub(crate) observer_router: &'a mut ObserverRouter,
}

#[derive(Clone, Copy)]
pub(crate) struct EngineRef<'a> {
    pub(crate) core: &'a CoreState,
    pub(crate) runtime: &'a EngineRuntime,
    pub(crate) hooks: &'a HookRuntimeState,
    pub(crate) task_scope: &'a TaskScope,
    pub(crate) observer_router: &'a ObserverRouter,
}

impl EngineMut<'_> {
    pub(crate) fn as_ref(&self) -> EngineRef<'_> {
        EngineRef {
            core: self.core,
            runtime: self.runtime,
            hooks: self.hooks,
            task_scope: self.task_scope,
            observer_router: self.observer_router,
        }
    }
}
impl Deref for EngineMut<'_> {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        self.core
    }
}
impl DerefMut for EngineMut<'_> {
    fn deref_mut(&mut self) -> &mut CoreState {
        self.core
    }
}
impl Deref for EngineRef<'_> {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        self.core
    }
}
