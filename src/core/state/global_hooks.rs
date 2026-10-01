//! 공용 busy 타이머에서 글로벌 훅 조건을 검사하고 명령 실행을 요청한다.

use crate::runtime::engine_access::EngineMut;

impl EngineMut<'_> {
    pub(crate) fn poll_global_hooks(&mut self) {
        self.hooks.run_due_global_hooks();
    }
}
