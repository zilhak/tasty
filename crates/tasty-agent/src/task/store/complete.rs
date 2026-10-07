//! 실행 결과 보고를 한 번의 쓰기로 확정한다.
//!
//! v2 task 는 결과 확정(출력 검증 포함)과 종결 전이를 같은 레코드 쓰기로 한다. 쓰기가
//! 실패하면 상태도 결과도 바뀌지 않고, 하류 readiness·fallback 도 움직이지 않는다. 쓰기 뒤
//! 후속 효과가 실패하면 오류를 돌려주고, 같은 보고를 다시 내면 후속 효과만 다시 적용한다.
//! 다시 낼 보고가 없는 재시작 뒤에는 [`TaskStore::resettle_waiting`] 이 남은 Waiting 을 마무리한다.

use super::super::attempt::{Completion, CompletionOutcome, CompletionReceipt, CompletionRecord};
use super::super::{Task, TaskId, TaskState, is_valid_transition};
use super::postprocess::PostprocessStep;
use super::{TaskStore, WorkspaceId, record_result, settle_typed_terminal};
use crate::{AgentError, CompletionRejection, Result};

#[cfg(test)]
thread_local! {
    /// 시험 전용: 다음 완료 쓰기를 실패시킨다.
    pub(crate) static FAIL_COMPLETION_PUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl TaskStore<'_> {
    /// 실행 결과를 보고한다. runner·훅·외부 보고·재시작 복구가 모두 이 경로를 쓴다.
    ///
    /// v2 task:
    /// - 보고의 회차가 지금 회차와 다르면 거절한다(`stale_attempt`).
    /// - 이미 끝난 회차에 같은 보고가 오면 같은 결과를 돌려준다(`duplicate`). 다른 보고면
    ///   거절한다(`different_report`). 회차 지문이 없는 종결 task 도 거절한다(`already_terminal`).
    /// - Running 이 아니면 전이 오류다.
    ///
    /// v1 task 는 결과를 쓴 뒤 상태를 전이한다. 결과 쓰기가 실패하면 상태를 바꾸지 않는다.
    pub fn complete(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        completion: Completion,
        now_ms: u64,
    ) -> Result<CompletionReceipt> {
        let task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        let requested = completion.outcome.state();
        if !task.is_typed() {
            if !is_valid_transition(&task.state, &requested) {
                return Err(transition_error(&task, &requested));
            }
            self.set_result(workspace_id, id, completion.result)?;
            let (task, transitioned) = self.set_state(workspace_id, id, requested, now_ms)?;
            return Ok(CompletionReceipt {
                task,
                transitioned,
                duplicate: false,
            });
        }

        let current = task.attempt.as_ref().map(|a| a.id.clone());
        if let Some(reported) = &completion.attempt_id
            && current.as_ref() != Some(reported)
        {
            return Err(rejected(
                &task,
                completion.attempt_id.clone(),
                CompletionRejection::StaleAttempt,
            ));
        }
        let digest = completion.digest();
        if task.state.is_terminal() {
            let done = task.attempt.as_ref().and_then(|a| a.completion.as_ref());
            return match done {
                Some(record) if record.digest == digest => {
                    // 첫 보고의 후속 효과가 중간에 끊겼을 수 있어 다시 적용한다.
                    let transitioned = self.propagate_transition(workspace_id, &task, now_ms)?;
                    Ok(CompletionReceipt {
                        task,
                        transitioned,
                        duplicate: true,
                    })
                }
                Some(_) => Err(rejected(
                    &task,
                    completion.attempt_id.clone(),
                    CompletionRejection::DifferentReport,
                )),
                None => Err(rejected(
                    &task,
                    completion.attempt_id.clone(),
                    CompletionRejection::AlreadyTerminal,
                )),
            };
        }
        if !matches!(task.state, TaskState::Running) {
            return Err(transition_error(&task, &requested));
        }

        let mut task = task;
        // 결과를 회수할 수 없으면 결과·경로를 정하지 않고 Unknown 으로 둔다. 종결이 아니라
        // 하류·fallback 은 그대로 기다리고, 회차 지문도 남기지 않는다(retry 가 새 회차를 연다).
        if matches!(completion.outcome, CompletionOutcome::Lost { .. }) {
            task.state = requested;
            super::super::postprocess::close_phase(&mut task);
            self.put(&task)?;
            return Ok(CompletionReceipt {
                task,
                transitioned: Vec::new(),
                duplicate: false,
            });
        }
        match self.postprocess_step(&mut task, &completion, now_ms)? {
            PostprocessStep::Continue(receipt) => return Ok(*receipt),
            PostprocessStep::Finalize => {}
            PostprocessStep::NotApplicable => record_result(&mut task, completion.result),
        }
        let state = settle_typed_terminal(&mut task, requested);
        let state = self.settle_route(&mut task, state);
        task.state = state;
        task.finished_at = Some(now_ms);
        if let Some(attempt) = task.attempt.as_mut() {
            attempt.completion = Some(CompletionRecord { digest });
        }
        #[cfg(test)]
        if FAIL_COMPLETION_PUT.with(|f| f.replace(false)) {
            return Err(AgentError::InvalidArgument(
                "injected completion failure".into(),
            ));
        }
        self.put(&task)?;
        let transitioned = self.propagate_transition(workspace_id, &task, now_ms)?;
        Ok(CompletionReceipt {
            task,
            transitioned,
            duplicate: false,
        })
    }

    /// workspace 의 모든 Waiting task 를 다시 평가한다. 완료 쓰기 뒤 하류 반영이 끊겼거나
    /// 그래프 레코드를 쓴 뒤 readiness 반영 전에 멈춘 경우를 재시작 때 마무리한다. 이미 맞는
    /// 상태라면 아무것도 바꾸지 않는다. 반환값은 상태가 바뀐 task 다.
    pub fn resettle_waiting(
        &mut self,
        workspace_id: WorkspaceId,
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let mut all = self.list(workspace_id)?;
        let ids: Vec<TaskId> = all
            .iter()
            .filter(|t| matches!(t.state, TaskState::Waiting))
            .map(|t| t.id.clone())
            .collect();
        let settled = self.settle_waiting(workspace_id, &mut all, &ids, now_ms)?;
        let mut changed = settled.clone();
        for t in settled.iter().filter(|t| t.state.is_terminal()) {
            changed.extend(self.cascade_downstream(workspace_id, &t.id, now_ms)?);
        }
        Ok(changed)
    }
}

fn transition_error(task: &Task, requested: &TaskState) -> AgentError {
    AgentError::InvalidTransition {
        from: task.state.name().to_string(),
        to: requested.name().to_string(),
    }
}

pub(super) fn rejected(
    task: &Task,
    reported: Option<String>,
    reason: CompletionRejection,
) -> AgentError {
    AgentError::CompletionRejected {
        task_id: task.id.clone(),
        attempt_id: reported,
        current_attempt_id: task.attempt.as_ref().map(|a| a.id.clone()),
        reason,
    }
}
