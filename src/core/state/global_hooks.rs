//! 공용 busy 타이머에서 글로벌 훅 조건을 검사하고 명령 실행을 요청한다.

use super::CoreState;
use crate::host_api::hooks::global::GlobalHookManager;

impl CoreState {
    pub(crate) fn poll_global_hooks(&mut self) {
        let to_fire = self.global_hook_manager.tick();
        for (_, command) in to_fire {
            GlobalHookManager::execute_command(&command);
        }
    }
}
