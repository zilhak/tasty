//! v2 task 의 실행 회차와 완료 보고.
//!
//! 회차는 Ready → Running 전이마다 새로 만든다. 완료 보고는 회차 id 를 지닐 수 있고,
//! 저장소는 결과 확정·종결 전이를 한 번의 쓰기로 하고 나서야 후속 readiness 를 갱신한다
//! ([`super::TaskStore::complete`]). 이미 끝난 회차에 같은 보고가 다시 오면 같은 응답을,
//! 다른 보고가 오면 충돌을 돌려준다. 다른 회차의 보고는 적용하지 않는다.

use serde::{Deserialize, Serialize};

use super::postprocess::{PostprocessOutcome, PostprocessProgress, PostprocessReport};
use super::{Task, TaskId, TaskResult, TaskState};

/// 한 실행 회차. `id` 는 `<task id>#<number>` 다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub id: String,
    pub number: u32,
    pub started_at: u64,
    /// 이 회차를 끝낸 보고. 같은 보고의 재전송을 알아보는 데 쓴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionRecord>,
    /// 본 작업이 성공한 뒤의 후처리 진행. 후처리가 없거나 본 작업이 끝나기 전이면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postprocess: Option<PostprocessProgress>,
}

/// 회차를 끝낸 보고의 요약. 보고 원문은 결과에 이미 있으므로 지문만 둔다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRecord {
    /// 보고 내용(결과·종결 종류)의 FNV-1a 64 지문, 16자리 16진수.
    pub digest: String,
}

/// `task_id` 의 `number` 번째 회차 id.
pub fn attempt_id(task_id: &TaskId, number: u32) -> String {
    format!("{task_id}#{number}")
}

/// 다음 Running 전이에서 만들 회차. 회차를 쓰지 않는 v1 task 는 없다.
pub fn next_attempt(task: &Task, now_ms: u64) -> Option<TaskAttempt> {
    if !task.is_typed() {
        return None;
    }
    let number = task.attempt.as_ref().map_or(1, |a| a.number + 1);
    Some(TaskAttempt {
        id: attempt_id(&task.id, number),
        number,
        started_at: now_ms,
        completion: None,
        postprocess: None,
    })
}

/// 보고한 종결 종류.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionOutcome {
    Succeeded,
    Failed { error: String },
}

impl CompletionOutcome {
    pub fn state(&self) -> TaskState {
        match self {
            CompletionOutcome::Succeeded => TaskState::Succeeded,
            CompletionOutcome::Failed { error } => TaskState::Failed {
                error: error.clone(),
            },
        }
    }
}

/// 실행 결과 보고. runner·훅·외부 보고·재시작 복구가 같은 형식으로 낸다.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// 보고가 속한 회차. 없으면 지금 진행 중인 회차로 본다.
    pub attempt_id: Option<String>,
    pub result: TaskResult,
    pub outcome: CompletionOutcome,
    /// 후처리 실행 보고. 있으면 본 작업이 아니라 후처리의 완료다.
    pub postprocess: Option<PostprocessReport>,
}

impl Completion {
    pub fn succeeded(attempt_id: Option<String>, result: TaskResult) -> Self {
        Self {
            attempt_id,
            result,
            outcome: CompletionOutcome::Succeeded,
            postprocess: None,
        }
    }

    /// 실패 보고. 결과의 `error` 에도 같은 사유를 싣는다.
    pub fn failed(attempt_id: Option<String>, error: String) -> Self {
        Self {
            attempt_id,
            result: TaskResult {
                exit_code: None,
                output: None,
                error: Some(error.clone()),
            },
            outcome: CompletionOutcome::Failed { error },
            postprocess: None,
        }
    }

    /// 후처리 실행 보고. 결과는 후처리 실패 사유만 싣는다(본 작업 결과는 회차에 있다).
    pub fn postprocessed(attempt_id: Option<String>, report: PostprocessReport) -> Self {
        let (outcome, error) = match &report.outcome {
            PostprocessOutcome::Collected { .. } => (CompletionOutcome::Succeeded, None),
            PostprocessOutcome::Failed { cause, message } => {
                let error = format!("postprocess {}: {message}", cause.name());
                (
                    CompletionOutcome::Failed {
                        error: error.clone(),
                    },
                    Some(error),
                )
            }
        };
        Self {
            attempt_id,
            result: TaskResult {
                exit_code: None,
                output: None,
                error,
            },
            outcome,
            postprocess: Some(report),
        }
    }

    /// 보고 내용의 지문. 회차 id 는 넣지 않는다(같은 회차 안에서만 비교한다).
    pub fn digest(&self) -> String {
        let mut body = serde_json::json!({"result": self.result, "outcome": self.outcome});
        if let Some(report) = &self.postprocess {
            body["postprocess"] = serde_json::to_value(report).unwrap_or_default();
        }
        format!("{:016x}", fnv1a64(body.to_string().as_bytes()))
    }
}

/// 완료 처리 결과.
#[derive(Debug, Clone)]
pub struct CompletionReceipt {
    /// 완료한 task 의 현재 레코드.
    pub task: Task,
    /// 이 완료로 상태가 바뀐 다른 task(fallback·하류).
    pub transitioned: Vec<Task>,
    /// 이미 같은 보고로 끝난 회차였다.
    pub duplicate: bool,
}

/// 프로세스·Rust 버전과 무관하게 같은 값을 내는 64비트 FNV-1a.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_is_stable_and_separates_different_reports() {
        // 고정 벡터: 빈 입력의 FNV-1a 64 는 오프셋 기저다.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        let ok = |code| {
            Completion::succeeded(
                None,
                TaskResult {
                    exit_code: Some(code),
                    output: None,
                    error: None,
                },
            )
        };
        assert_eq!(ok(0).digest(), ok(0).digest());
        assert_ne!(ok(0).digest(), ok(1).digest());
        assert_ne!(
            ok(0).digest(),
            Completion::failed(None, "x".into()).digest()
        );
        // 회차 id 는 지문에 들지 않는다.
        let mut with_id = ok(0);
        with_id.attempt_id = Some("t#1".into());
        assert_eq!(with_id.digest(), ok(0).digest());
    }
}
