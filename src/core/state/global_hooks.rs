//! 글로벌 훅(`tasty set global-hook`) 조건 평가/발화. `poll_global_hooks` 는
//! `AppEvent::BusyPoll` 1Hz cadence 에 편승해 호출된다 — busy_tick/attach_tick
//! 처럼 전용 ticker 스레드를 새로 두지 않고, 이미 모든 gui/headless 부팅 경로에 배선된
//! 1Hz 이벤트를 그대로 재사용한다.

use super::CoreState;
use crate::host_api::hooks::global::GlobalHookManager;

impl CoreState {
    /// 등록된 글로벌 훅 중 조건(`interval:SECS`/`once:SECS`)이 찬 것을 찾아 실행한다.
    pub(crate) fn poll_global_hooks(&mut self) {
        let to_fire = self.global_hook_manager.tick();
        for (_, command) in to_fire {
            GlobalHookManager::execute_command(&command);
        }
    }
}
