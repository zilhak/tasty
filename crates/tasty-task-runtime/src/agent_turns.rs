//! agent task 회차와 세션(surface)의 턴 연결.
//!
//! 러너가 지시를 보내기 전에 surface 를 회차에 묶고, provider 플러그인이 턴의 시작·종료를
//! 보고하면 이 표에 모은다. 종결은 러너가 이 표를 읽어 완료 보고로 낸다(호스트가 종결을 독점).
//! 표는 영속하지 않는다. 재시작하면 턴의 귀속을 알 수 없어 러너가 그 회차를 실패로 정리한다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use tasty_agent::task::FailureCode;
use tasty_agent::task::agent::report;
use tasty_agent::{SubmissionRejection, TaskId, TaskResult};

const WHAT: &str = "agent turn registry";
static POISONED: AtomicBool = AtomicBool::new(false);

/// surface 하나에 묶인 회차 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnBinding {
    pub workspace: u32,
    pub task: TaskId,
    pub attempt: String,
    pub provider: String,
    /// 이 회차의 턴이 시작됐다. 시작 전의 종료 보고는 이전 턴의 것이라 받지 않는다.
    pub armed: bool,
    pub ended: Option<TurnEnd>,
    /// 같은 회차의 명시 제출(검증을 마친 wire 값).
    pub submitted: Option<Value>,
    /// 세션이 입력을 기다리기 시작한 시각. 회차 기록에 쓴 값과 같다.
    pub awaiting_since: Option<u64>,
    /// 회차에 세션 연결을 기록했다.
    pub linked: bool,
}

impl TurnBinding {
    pub fn new(
        workspace: u32,
        task: TaskId,
        attempt: String,
        provider: String,
        armed: bool,
    ) -> Self {
        Self {
            workspace,
            task,
            attempt,
            provider,
            armed,
            ended: None,
            submitted: None,
            awaiting_since: None,
            linked: false,
        }
    }
}

/// 턴이 끝난 방식.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEnd {
    /// 정상 종료. provider 가 보고한 마지막 assistant 메시지(없을 수 있다).
    Answer(Option<String>),
    /// provider 가 턴을 오류로 끝냈다.
    Error(String),
}

/// provider 의 턴 보고.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    Started,
    Ended(TurnEnd),
}

/// 보고를 적용한 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportOutcome {
    /// surface 에 묶인 회차가 없다.
    Unbound,
    Applied {
        task: TaskId,
        attempt: String,
    },
    /// 묶인 회차가 있지만 적용하지 않았다.
    Ignored(&'static str),
}

/// 제출을 받은 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    /// 같은 값의 재제출이다.
    Duplicate,
}

#[derive(Default)]
pub struct AgentTurns {
    inner: Mutex<HashMap<u32, TurnBinding>>,
}

impl AgentTurns {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, TurnBinding>> {
        tasty_utils::poison::recover_mutex(self.inner.lock(), WHAT, &POISONED)
    }

    /// surface 를 회차에 묶는다. 다른 회차가 이미 묶여 있으면 그 task 를 돌려주고 묶지 않는다.
    /// 같은 회차를 다시 묶으면 그대로 둔다.
    pub fn bind(&self, surface: u32, binding: TurnBinding) -> Result<(), TaskId> {
        let mut g = self.lock();
        match g.get(&surface) {
            Some(b) if b.task == binding.task && b.attempt == binding.attempt => Ok(()),
            Some(b) => Err(b.task.clone()),
            None => {
                g.insert(surface, binding);
                Ok(())
            }
        }
    }

    /// surface 를 묶고 있는 task.
    pub fn holder(&self, surface: u32) -> Option<TaskId> {
        self.lock().get(&surface).map(|b| b.task.clone())
    }

    /// task 의 회차가 묶은 surface 를 푼다. 늦게 온 보고는 이후 적용되지 않는다.
    pub fn release(&self, task: &TaskId) {
        self.lock().retain(|_, b| b.task != *task);
    }

    pub fn get(&self, surface: u32) -> Option<TurnBinding> {
        self.lock().get(&surface).cloned()
    }

    /// task 가 묶은 surface 와 그 상태.
    pub fn find(&self, task: &TaskId) -> Option<(u32, TurnBinding)> {
        self.lock()
            .iter()
            .find(|(_, b)| b.task == *task)
            .map(|(s, b)| (*s, b.clone()))
    }

    /// 입력 대기 여부를 반영한다. 회차 기록을 다시 써야 하면(처음이거나 대기가 바뀜)
    /// 기록할 대기 시작 시각을 돌려준다.
    pub fn note_awaiting(&self, task: &TaskId, awaiting: bool, now_ms: u64) -> Option<Option<u64>> {
        let mut g = self.lock();
        let b = g.values_mut().find(|b| b.task == *task)?;
        let next = match (awaiting, b.awaiting_since) {
            (true, Some(t)) => Some(t),
            (true, None) => Some(now_ms),
            (false, _) => None,
        };
        if b.linked && next == b.awaiting_since {
            return None;
        }
        b.linked = true;
        b.awaiting_since = next;
        Some(next)
    }

    /// `provider` 플러그인의 보고를 적용한다. 다른 provider 의 회차에는 적용하지 않는다.
    pub fn report(&self, surface: u32, provider: &str, event: TurnEvent) -> ReportOutcome {
        let mut g = self.lock();
        let Some(b) = g.get_mut(&surface) else {
            return ReportOutcome::Unbound;
        };
        if b.provider != provider {
            return ReportOutcome::Ignored("the surface is bound to another provider's turn");
        }
        if b.ended.is_some() {
            return ReportOutcome::Ignored("the bound turn already ended");
        }
        match event {
            TurnEvent::Started => b.armed = true,
            TurnEvent::Ended(_) if !b.armed => {
                return ReportOutcome::Ignored("the bound turn has not started yet");
            }
            TurnEvent::Ended(end) => b.ended = Some(end),
        }
        ReportOutcome::Applied {
            task: b.task.clone(),
            attempt: b.attempt.clone(),
        }
    }

    /// 검증을 마친 결과를 회차에 제출한다. 거절하면 사유와 지금 회차를 돌려준다. 첫 제출을 유지하고, 같은 값은 다시 받으며, 다른 값은
    /// 거절한다. 턴이 끝난 뒤의 제출은 받지 않는다(확정할 결과가 바뀌지 않게).
    pub fn submit(
        &self,
        task: &TaskId,
        attempt: &str,
        value: Value,
    ) -> Result<SubmitOutcome, (SubmissionRejection, Option<String>)> {
        let mut g = self.lock();
        let Some(b) = g.values_mut().find(|b| b.task == *task) else {
            return Err((SubmissionRejection::NotRunning, None));
        };
        if b.attempt != attempt {
            return Err((SubmissionRejection::StaleAttempt, Some(b.attempt.clone())));
        }
        if b.ended.is_some() {
            return Err((SubmissionRejection::TurnEnded, Some(b.attempt.clone())));
        }
        match &b.submitted {
            Some(v) if *v == value => Ok(SubmitOutcome::Duplicate),
            Some(_) => Err((SubmissionRejection::Conflict, Some(b.attempt.clone()))),
            None => {
                b.submitted = Some(value);
                Ok(SubmitOutcome::Accepted)
            }
        }
    }
}

/// 한 번의 poll 판단.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnPoll {
    /// 턴이 진행 중이다. `awaiting_input` 이면 세션이 사용자 입력을 기다린다.
    Active {
        awaiting_input: bool,
    },
    Done(TaskResult),
    Failed(String),
}

/// 묶인 회차와 provider 상태로 다음 단계를 정한다.
///
/// - 턴이 끝났으면 제출 → 최종 답변 순서로 결과를 고른다. 출력이 제출을 요구하는데 없으면
///   `result_missing` 이다. idle 상태만으로는 끝내지 않는다(턴 종료 보고가 있어야 한다).
/// - 끝나지 않았으면 세션 종료는 `agent_exited`, 기한 경과는 `timed_out` 이고, needs_input 은
///   대기로 남는다.
pub fn decide(
    binding: &TurnBinding,
    surface: u32,
    needs_submission: bool,
    provider_state: Option<&str>,
    now_ms: u64,
    deadline_ms: Option<u64>,
) -> TurnPoll {
    match &binding.ended {
        Some(TurnEnd::Error(e)) => TurnPoll::Failed(FailureCode::AgentTurnError.message(e)),
        Some(TurnEnd::Answer(answer)) => {
            if binding.submitted.is_none() && (needs_submission || answer.is_none()) {
                let why = if needs_submission {
                    "the turn ended without submitting the required result (tasty agent task-submit)"
                } else {
                    "the turn ended without a final answer"
                };
                return TurnPoll::Failed(FailureCode::ResultMissing.message(why));
            }
            let mut out = json!({
                report::PROVIDER: binding.provider,
                report::SURFACE_ID: surface,
                report::FINAL_ANSWER: answer,
            });
            if let Some(v) = &binding.submitted {
                out[report::SUBMITTED] = v.clone();
            }
            TurnPoll::Done(TaskResult {
                exit_code: None,
                output: Some(out),
                error: None,
            })
        }
        None => {
            if provider_state == Some("exited") {
                return TurnPoll::Failed(
                    FailureCode::AgentExited
                        .message(format!("surface {surface} exited before the turn ended")),
                );
            }
            if deadline_ms.is_some_and(|d| now_ms >= d) {
                return TurnPoll::Failed(
                    FailureCode::TimedOut.message("the agent turn did not end before timeout_ms"),
                );
            }
            TurnPoll::Active {
                awaiting_input: provider_state == Some("needs_input"),
            }
        }
    }
}

#[cfg(test)]
#[path = "agent_turns_tests.rs"]
mod tests;
