//! IpcSequence·셸 실행의 결과. 수동 dispatch는 호출자 조회 결과로, 자동 실행은 알림으로 전달한다.

use serde_json::{Value, json};
use tasty_ipc::host_call::InjectError;
use tasty_ipc::protocol::{ERR_EXPIRED_BEFORE_RUN, ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN};

/// 한 스텝의 결과. 응답 대기 상한이 지난 스텝은 성공으로 보지 않고 결과 불명으로 남긴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Ok,
    /// 명령이 실행되어 오류로 끝났다.
    Failed(String),
    /// 명령이 실행되지 않았다.
    NotRun(String),
    /// 응답을 기다리다 포기했다. 명령은 나중에 실행될 수도 있다.
    Unknown(String),
}

impl StepStatus {
    pub fn from_error(e: &InjectError) -> Self {
        let text = e.to_string();
        match e {
            InjectError::Rpc { code, .. } if *code == ERR_EXPIRED_BEFORE_RUN => Self::NotRun(text),
            InjectError::Rpc { code, .. } if *code == ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN => {
                Self::Unknown(text)
            }
            InjectError::Rpc { .. } => Self::Failed(text),
            InjectError::Timeout(_) | InjectError::Disconnected => Self::Unknown(text),
            InjectError::Refused(_) | InjectError::Send(_) | InjectError::Expired(_) => {
                Self::NotRun(text)
            }
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed(_) => "failed",
            Self::NotRun(_) => "not_run",
            Self::Unknown(_) => "unknown",
        }
    }

    fn error(&self) -> Option<&str> {
        match self {
            Self::Ok => None,
            Self::Failed(e) | Self::NotRun(e) | Self::Unknown(e) => Some(e),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub index: usize,
    pub method: String,
    pub status: StepStatus,
}

/// 실행 전체의 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Succeeded,
    /// 일부 스텝만 성공했다.
    Partial,
    /// 성공한 스텝이 없다.
    Failed,
    /// 실패는 없지만 결과를 모르는 스텝이 있다.
    Unknown,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceOutcome {
    pub steps: Vec<StepResult>,
}

impl SequenceOutcome {
    pub fn verdict(&self) -> Verdict {
        let ok = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Ok)
            .count();
        let bad = self
            .steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Failed(_) | StepStatus::NotRun(_)))
            .count();
        match (ok, bad) {
            (_, 0) if ok == self.steps.len() => Verdict::Succeeded,
            (_, 0) => Verdict::Unknown,
            (0, _) => Verdict::Failed,
            _ => Verdict::Partial,
        }
    }

    /// 성공하지 않은 스텝을 사람이 읽는 한 줄로 묶는다. 모두 성공했으면 빈 문자열이다.
    pub fn failure_summary(&self) -> String {
        self.steps
            .iter()
            .filter_map(|s| {
                let error = s.status.error()?;
                let step = format!("{} ({})", s.index + 1, s.method);
                let key = match s.status {
                    StepStatus::Failed(_) => "hook_notice.step_failed",
                    StepStatus::NotRun(_) => "hook_notice.step_not_run",
                    _ => "hook_notice.step_unknown",
                };
                Some(crate::i18n::t_fmt2(key, &step, error))
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// 에이전트 응답용 한 줄. 표시 언어와 관계없이 영어로 고정한다.
    pub fn result_line(&self) -> String {
        let total = self.steps.len();
        let ok = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Ok)
            .count();
        let problems: Vec<String> = self
            .steps
            .iter()
            .filter_map(|s| {
                let error = s.status.error()?;
                let what = match s.status {
                    StepStatus::Failed(_) => "failed",
                    StepStatus::NotRun(_) => "did not run",
                    _ => "gave no answer in time (result unknown)",
                };
                Some(format!(
                    "step {} ({}) {what}: {error}",
                    s.index + 1,
                    s.method
                ))
            })
            .collect();
        if problems.is_empty() {
            format!("{} of {total} steps succeeded", ok)
        } else {
            format!("{} of {total} steps succeeded; {}", ok, problems.join("; "))
        }
    }

    pub fn steps_json(&self) -> Value {
        Value::Array(
            self.steps
                .iter()
                .map(|s| {
                    json!({
                        "step": s.index + 1,
                        "method": s.method,
                        "status": s.status.key(),
                        "error": s.status.error(),
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(index: usize, status: StepStatus) -> StepResult {
        StepResult {
            index,
            method: format!("m{index}"),
            status,
        }
    }

    #[test]
    fn a_failed_step_is_never_reported_as_success() {
        let outcome = |s: Vec<StepStatus>| SequenceOutcome {
            steps: s.into_iter().enumerate().map(|(i, s)| step(i, s)).collect(),
        };
        let fail = || StepStatus::Failed("rpc error -32601: no such method".into());
        assert_eq!(
            outcome(vec![StepStatus::Ok, StepStatus::Ok]).verdict(),
            Verdict::Succeeded
        );
        assert_eq!(
            outcome(vec![StepStatus::Ok, fail()]).verdict(),
            Verdict::Partial
        );
        assert_eq!(outcome(vec![fail(), fail()]).verdict(), Verdict::Failed);
        assert_eq!(
            outcome(vec![StepStatus::Ok, StepStatus::Unknown("timeout".into())]).verdict(),
            Verdict::Unknown
        );
        let partial = outcome(vec![StepStatus::Ok, fail()]);
        assert!(partial.failure_summary().contains("2 (m1)"));
        assert_eq!(partial.steps_json()[1]["status"], "failed");
        assert!(
            partial
                .result_line()
                .starts_with("1 of 2 steps succeeded; step 2 (m1) failed")
        );
    }

    #[test]
    fn inject_errors_map_to_ran_not_run_or_unknown() {
        use std::time::Duration;
        assert!(matches!(
            StepStatus::from_error(&InjectError::Timeout(Duration::from_secs(10))),
            StepStatus::Unknown(_)
        ));
        assert!(matches!(
            StepStatus::from_error(&InjectError::Expired(Duration::from_secs(10))),
            StepStatus::NotRun(_)
        ));
        assert!(matches!(
            StepStatus::from_error(&InjectError::Rpc {
                code: -32601,
                message: "Method not found".into()
            }),
            StepStatus::Failed(_)
        ));
        assert!(matches!(
            StepStatus::from_error(&InjectError::Rpc {
                code: ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN,
                message: "timeout".into()
            }),
            StepStatus::Unknown(_)
        ));
    }
}
