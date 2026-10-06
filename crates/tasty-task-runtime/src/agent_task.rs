//! agent task 의 결과 제출과 provider 턴 보고. 둘 다 회차에 결과를 제안할 뿐이고, 검증과
//! 종결은 러너가 턴 표를 읽어 완료 보고로 한다.

use serde_json::Value;
use tasty_agent::task::contract::{FailureStage, TaskFailure};
use tasty_agent::{AgentError, SubmissionRejection, TaskCommand, TaskId, TaskState, TaskStore};
use tasty_memory::HOST_OWNER;

use crate::agent_turns::{ReportOutcome, SubmitOutcome, TurnEvent};
use crate::{TaskScope, TaskService};

/// 제출한 쪽. 세션 토큰을 가진 agent 는 자기 세션의 회차에만 낼 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitter {
    /// 사용자 CLI 또는 권한 검사를 통과한 플러그인.
    Trusted,
    /// 세션 토큰으로 확인한 agent 세션. 값은 그 세션의 surface 다.
    Session(u32),
}

impl TaskService {
    /// agent task 회차에 결과를 제출한다. 출력 타입으로 바로 검증하고, 받은 값은 턴이 끝날 때
    /// 러너가 결과로 확정한다. 제출 성공은 task 의 성공이 아니다.
    pub fn agent_submit_result(
        &self,
        scope: &TaskScope,
        workspace_id: u32,
        task_id: &TaskId,
        attempt_id: &str,
        output: &Value,
        submitter: Submitter,
    ) -> Result<SubmitOutcome, AgentError> {
        let seq = scope.agent_seq().clone();
        let task = self
            .with_memory(|mem| {
                TaskStore::new(mem, HOST_OWNER, seq.as_ref()).get(workspace_id, task_id)
            })?
            .ok_or_else(|| AgentError::TaskNotFound(task_id.clone()))?;
        let current = task.attempt.as_ref().map(|a| a.id.clone());
        let reject = |reason, current_attempt_id: Option<String>| AgentError::SubmissionRejected {
            task_id: task_id.clone(),
            attempt_id: attempt_id.to_string(),
            current_attempt_id,
            reason,
        };
        let contract = match (&task.command, &task.contract, &task.state) {
            (TaskCommand::Agent { .. }, Some(c), TaskState::Running) => c,
            _ => return Err(reject(SubmissionRejection::NotRunning, current)),
        };
        if current.as_deref() != Some(attempt_id) {
            return Err(reject(SubmissionRejection::StaleAttempt, current));
        }
        let Some((surface, _)) = self.agent_turns().find(task_id) else {
            return Err(reject(SubmissionRejection::NotRunning, current));
        };
        if matches!(submitter, Submitter::Session(s) if s != surface) {
            return Err(reject(SubmissionRejection::NotTheSession, current));
        }
        let schema = contract.output_schema(&task.command);
        let typed = contract
            .defs()
            .validate_typed(&schema, output)
            .map_err(|e| {
                AgentError::TypeContract(Box::new(TaskFailure::typed(
                    FailureStage::OutputValidation,
                    Some(task_id.clone()),
                    e,
                )))
            })?;
        self.agent_turns()
            .submit(task_id, attempt_id, typed.to_wire())
            .map_err(|(reason, current)| reject(reason, current))
    }

    /// `provider` 플러그인이 보고한 턴의 시작·종료를 그 surface 에 묶인 회차에 적용한다.
    pub fn agent_turn_report(
        &self,
        surface: u32,
        provider: &str,
        event: TurnEvent,
    ) -> ReportOutcome {
        self.agent_turns().report(surface, provider, event)
    }
}
