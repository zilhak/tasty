//! 결과 전 task 레코드 상한을 실행 직전에 적용한다. 상한이 생기기 전에 저장된 정의와, 출력·lease
//! 치환으로 커진 command 는 결과를 줄여도 저장할 몫이 없으므로 실행하지 않는다.

use tasty_agent::Task;
use tasty_agent::task::record_limit;

use super::HostExecutor;

impl HostExecutor {
    /// 레코드가 memory 값 상한에 맞춘 결과 전 상한 안인가. 넘으면 사유를 돌려준다.
    pub(super) fn record_fits(&self, task: &Task, what: &str) -> Result<(), String> {
        let limit = self
            .ctx
            .with_memory(|mem| record_limit::limit_for(mem.config().entry_max_bytes));
        record_limit::check(task, what, limit)
    }

    /// 출력·lease 를 치환한 task 가 상한 안이면 치환 여부(`changed`)를 그대로 돌려준다.
    pub(super) fn substituted_fits(&self, task: &Task, changed: bool) -> Result<bool, String> {
        self.record_fits(task, "the command with substituted values")
            .map(|()| changed)
    }
}
