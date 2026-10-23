//! 러너의 완료 보고와, 결과를 받지 못한 보고의 재전송 처리.

use tasty_agent::TaskId;
use tasty_agent::runner::completion_retryable;
use tasty_agent::task::Completion;

use super::RunnerContext;

/// 러너의 완료 보고. 결과를 받지 못한 보고를 기억했다가, 다시 낸 같은 보고가 앞 보고의 확정을 알려
/// 주면 미확인 종결 의무를 다시 확인한다([`RunnerContext::complete_task_again`]).
#[derive(Default)]
pub(super) struct CompletionReports {
    unanswered: std::collections::HashSet<TaskId>,
}

impl CompletionReports {
    pub(super) fn report(
        &mut self,
        ctx: &RunnerContext,
        workspace_id: u32,
        task_id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<(), tasty_agent::AgentError> {
        let after_unanswered = self.unanswered.remove(task_id);
        match ctx.complete_task_again(workspace_id, task_id, completion, now_ms, after_unanswered) {
            Ok(_) => Ok(()),
            Err(e) => {
                // 러너가 같은 보고를 다시 내는 오류만 기억한다.
                if completion_retryable(&e) {
                    self.unanswered.insert(task_id.clone());
                }
                Err(e)
            }
        }
    }
}
