//! v2 task 의 실행 회차와 완료 보고.
//!
//! 회차는 Ready → Running 전이마다 새로 만든다. 완료 보고는 회차 id 를 지닐 수 있고,
//! 저장소는 결과 확정·종결 전이를 한 번의 쓰기로 하고 나서야 후속 readiness 를 갱신한다
//! ([`super::TaskStore::complete`]). 이미 끝난 회차에 같은 보고가 다시 오면 같은 응답을,
//! 다른 보고가 오면 충돌을 돌려준다. 다른 회차의 보고는 적용하지 않는다.

use serde::{Deserialize, Serialize};

use super::postprocess::{
    PostprocessCause, PostprocessOutcome, PostprocessProgress, PostprocessReport,
};
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
    /// agent task 회차가 지시를 보낸 세션과 입력 대기 상태.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<super::agent::AgentLink>,
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
        agent: None,
    })
}

/// 보고한 종결 종류.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionOutcome {
    Succeeded,
    Failed {
        error: String,
    },
    /// 실행 결과를 회수할 수 없다. task 는 Unknown 으로 남고 하류·fallback 은 움직이지 않는다.
    Lost {
        reason: String,
    },
}

impl CompletionOutcome {
    pub fn state(&self) -> TaskState {
        match self {
            CompletionOutcome::Succeeded => TaskState::Succeeded,
            CompletionOutcome::Failed { error } => TaskState::Failed {
                error: error.clone(),
            },
            CompletionOutcome::Lost { reason } => TaskState::Unknown {
                reason: Some(reason.clone()),
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
    /// 레코드에 넣으려고 회차 진행의 본 작업 결과 사본(후처리 입력으로 보관한 것과 그로 만드는
    /// `raw.execution`·`raw.execution_truncated`)을 비우고 확정하라는 표시. 후처리 보고에만 쓴다.
    pub main_copy_dropped: bool,
}

/// [`Completion::too_large_to_store`] 가 남기는 사유의 바이트 상한. 이보다 긴 사유를 실은 보고는
/// 기록하지 못했을 때 줄일 대상이고, 줄인 보고는 이 안이라 줄이기가 한 번에 끝난다.
pub const SHRUNK_ERROR_LIMIT: usize = 4 * 1024;

/// 잘린 사유 끝에 붙이는 표시.
const TRUNCATED_MARK: &str = "...(truncated)";

/// `text` 가 `cap` 바이트를 넘으면 표시를 포함해 `cap` 안으로 자른다. 자르는 자리는 UTF-8 문자
/// 경계다.
fn head_within(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut cut = cap.saturating_sub(TRUNCATED_MARK.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut head = text[..cut].to_owned();
    if head.len() + TRUNCATED_MARK.len() <= cap {
        head.push_str(TRUNCATED_MARK);
    }
    head
}

impl Completion {
    pub fn succeeded(attempt_id: Option<String>, result: TaskResult) -> Self {
        Self {
            attempt_id,
            result,
            outcome: CompletionOutcome::Succeeded,
            postprocess: None,
            main_copy_dropped: false,
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
            main_copy_dropped: false,
        }
    }

    /// 종료 코드·출력이 있는 실패 보고. 결과의 `error` 를 실패 사유로 쓴다.
    pub fn exited(attempt_id: Option<String>, result: TaskResult) -> Self {
        let error = result.error.clone().unwrap_or_default();
        Self {
            attempt_id,
            result,
            outcome: CompletionOutcome::Failed { error },
            postprocess: None,
            main_copy_dropped: false,
        }
    }

    /// 기록할 수 없을 만큼 큰 보고를 대신하는 같은 회차의 실패 보고. 출력을 버리고 사유의 첫
    /// 줄과 기록하지 못한 이유만 남긴다. 종료 코드는 유지한다. 사유는 [`SHRUNK_ERROR_LIMIT`]
    /// 안으로 자르므로 이 보고를 다시 줄일 일은 없다.
    pub fn too_large_to_store(&self, why: &str) -> Self {
        let error = match self.result.error.as_deref().and_then(|e| e.lines().next()) {
            Some(first) => {
                let tail = format!(" (the full result could not be stored: {why})");
                let first = head_within(first, SHRUNK_ERROR_LIMIT.saturating_sub(tail.len()));
                format!("{first}{tail}")
            }
            None => format!("the result could not be stored: {why}"),
        };
        // 이유 자체가 길어도 상한을 지킨다.
        let error = head_within(&error, SHRUNK_ERROR_LIMIT);
        Self {
            attempt_id: self.attempt_id.clone(),
            result: TaskResult {
                exit_code: self.result.exit_code,
                output: None,
                error: Some(error.clone()),
            },
            outcome: CompletionOutcome::Failed { error },
            postprocess: None,
            main_copy_dropped: false,
        }
    }

    /// 기록할 수 없을 만큼 큰 후처리 실행 보고를 대신하는 보고. 두 단계로 줄인다.
    ///
    /// 1. 성공(`Collected`) 보고면 먼저 같은 보고에 [`Completion::main_copy_dropped`] 를 세운다.
    ///    진단용 본 작업 결과 사본만 비우고 성공으로 확정한다.
    /// 2. 그래도 넘치거나 실패 보고면 같은 실행의 실패로 바꾼다. stdout·stderr 를 버리고 원인을
    ///    [`PostprocessCause::ResultTooLarge`] 로 둔다(이때도 사본을 비운다).
    ///
    /// 회차 진행과 맞도록 실행 번호와 종료 코드는 유지한다. 이미 2 단계인 보고면 `None` 이다(다시
    /// 바꿔도 줄지 않는다).
    pub fn postprocess_too_large_to_store(&self, why: &str) -> Option<Self> {
        let report = self.postprocess.as_ref()?;
        if report.cause() == Some(PostprocessCause::ResultTooLarge) {
            return None;
        }
        if report.cause().is_none() && !self.main_copy_dropped {
            return Some(Self {
                main_copy_dropped: true,
                ..self.clone()
            });
        }
        let message = head_within(
            &format!("the postprocess result could not be stored: {why}"),
            SHRUNK_ERROR_LIMIT,
        );
        let failed = Self::postprocessed(
            self.attempt_id.clone(),
            PostprocessReport {
                run: report.run,
                exit_code: report.exit_code,
                stderr: None,
                stderr_truncated: false,
                outcome: PostprocessOutcome::Failed {
                    cause: PostprocessCause::ResultTooLarge,
                    message,
                },
            },
        );
        Some(Self {
            main_copy_dropped: true,
            ..failed
        })
    }

    /// 결과를 회수할 수 없다는 보고. 결과의 `error` 에도 같은 사유를 싣는다.
    pub fn lost(attempt_id: Option<String>, reason: String) -> Self {
        Self {
            attempt_id,
            result: TaskResult {
                exit_code: None,
                output: None,
                error: Some(reason.clone()),
            },
            outcome: CompletionOutcome::Lost { reason },
            postprocess: None,
            main_copy_dropped: false,
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
            main_copy_dropped: false,
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

    /// 줄인 보고의 사유는 첫 줄이나 이유가 아무리 길어도 상한 안이다. 자르는 자리는 문자
    /// 경계이고 잘렸다는 표시를 남긴다.
    #[test]
    fn a_shrunk_reason_stays_within_the_limit() {
        let why = "memory: value too large: 1126687 bytes (max 1048576)";
        let tail = format!(" (the full result could not be stored: {why})");
        for first in ["e".repeat(1100 * 1024), "가".repeat(400 * 1024)] {
            for pad in 0..3 {
                let reason = format!("{}{first}\nsecond line", "a".repeat(pad));
                let shrunk = Completion::failed(None, reason.clone()).too_large_to_store(why);
                let error = shrunk.result.error.as_deref().expect("error");
                assert!(error.len() <= SHRUNK_ERROR_LIMIT, "{}", error.len());
                assert!(
                    error.ends_with(&format!("{TRUNCATED_MARK}{tail}")),
                    "{error}"
                );
                let head = &error[..error.len() - tail.len() - TRUNCATED_MARK.len()];
                assert!(reason.starts_with(head));
                assert!(head.len() + 3 > SHRUNK_ERROR_LIMIT - tail.len() - TRUNCATED_MARK.len());
                assert_eq!(
                    shrunk.outcome,
                    CompletionOutcome::Failed {
                        error: error.to_owned()
                    }
                );
            }
        }

        // 짧은 첫 줄은 그대로 둔다.
        let shrunk =
            Completion::failed(None, "Run exited with code 3\nx".into()).too_large_to_store(why);
        assert_eq!(
            shrunk.result.error.as_deref(),
            Some(format!("Run exited with code 3{tail}").as_str())
        );

        // 이유가 상한보다 길어도 사유 전체가 상한 안이다.
        let long_why = "w".repeat(2 * SHRUNK_ERROR_LIMIT);
        for c in [
            Completion::failed(None, "boom".into()),
            Completion::succeeded(
                None,
                TaskResult {
                    exit_code: None,
                    output: Some(serde_json::json!("x")),
                    error: None,
                },
            ),
        ] {
            let error = c.too_large_to_store(&long_why).result.error.expect("error");
            assert!(error.len() <= SHRUNK_ERROR_LIMIT, "{}", error.len());
            assert!(error.ends_with(TRUNCATED_MARK), "{error}");
        }
    }
}
