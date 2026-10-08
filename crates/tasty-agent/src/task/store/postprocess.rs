//! 후처리가 있는 v2 task 의 완료 보고.
//!
//! 본 작업의 성공 보고는 task 를 끝내지 않고 회차에 본 작업 결과와 후처리 진행을 기록한다.
//! 후처리 실행 보고는 재시도가 남았으면 다음 실행을 예약하고, 아니면 그 보고로 회차를 확정한다.
//! 재시도·대기 동안 task 는 Running 이며 종결과 `on_failure` 는 마지막 보고에서 한 번만 일어난다.

use super::super::attempt::{Completion, CompletionOutcome, CompletionReceipt};
use super::super::postprocess::{
    PostprocessOutcome, PostprocessPhase, PostprocessProgress, PostprocessRunSummary,
    PostprocessSpec, finalize_postprocessed, next_run,
};
use super::super::types::MAX_VALUE_BYTES;
use super::super::{Task, TaskCommand, TaskId, TaskResult, TaskState, contract};
use super::complete::rejected;
use super::{TaskStore, WorkspaceId};
use crate::{AgentError, CompletionRejection, Result};

/// 후처리 단계의 처리 결과.
pub(super) enum PostprocessStep {
    /// 후처리와 관계없는 보고다. 일반 확정 경로로 넘긴다.
    NotApplicable,
    /// task 는 Running 으로 남는다. 받은 보고를 기록했거나 같은 보고의 재전송이다.
    Continue(Box<CompletionReceipt>),
    /// 이 보고로 회차를 확정한다. task 에는 확정한 결과가 들어 있다.
    Finalize,
}

impl TaskStore<'_> {
    /// Running 인 v2 task 의 보고가 후처리 단계에 속하면 처리한다.
    pub(super) fn postprocess_step(
        &mut self,
        task: &mut Task,
        completion: &Completion,
        now_ms: u64,
    ) -> Result<PostprocessStep> {
        let Some(contract) = task.contract.clone() else {
            return Ok(PostprocessStep::NotApplicable);
        };
        let Some(spec) = contract.postprocess.clone() else {
            return Ok(PostprocessStep::NotApplicable);
        };
        let progress = task.attempt.as_ref().and_then(|a| a.postprocess.clone());
        match (&completion.postprocess, progress) {
            (None, None) => {
                if !matches!(completion.outcome, CompletionOutcome::Succeeded) {
                    // 본 작업이 실패했으면 후처리를 실행하지 않는다.
                    return Ok(PostprocessStep::NotApplicable);
                }
                if response_too_large(task, &completion.result) {
                    // 후처리 입력으로 쓸 응답은 자를 수 없다. 출력과 같은 값 상한을 넘으면 후처리 없이
                    // 일반 확정 경로에서 출력 검증 실패로 끝낸다. 회차에 응답을 저장하지도 않는다.
                    return Ok(PostprocessStep::NotApplicable);
                }
                let progress = PostprocessProgress {
                    execution: completion.result.clone(),
                    main_digest: completion.digest(),
                    phase: PostprocessPhase::Pending {
                        run: 1,
                        not_before_ms: now_ms,
                    },
                    failed_runs: Vec::new(),
                };
                self.keep_running(task, progress, false)
            }
            (None, Some(p)) => {
                if p.main_digest == completion.digest() {
                    self.keep_running(task, p, true)
                } else {
                    Err(rejected(
                        task,
                        completion.attempt_id.clone(),
                        CompletionRejection::DifferentReport,
                    ))
                }
            }
            (Some(_), None) => Err(AgentError::InvalidArgument(format!(
                "task {}: postprocess report before the main execution finished",
                task.id
            ))),
            (Some(report), Some(mut p)) => {
                if p.failed_runs.iter().any(|r| r.run == report.run) {
                    let same = p.failed_runs.iter().any(|r| {
                        r.run == report.run
                            && Some(r.cause) == report.cause()
                            && r.exit_code == report.exit_code
                    });
                    return if same {
                        self.keep_running(task, p, true)
                    } else {
                        Err(rejected(
                            task,
                            completion.attempt_id.clone(),
                            CompletionRejection::DifferentReport,
                        ))
                    };
                }
                // 예약만 된 실행의 보고는 프로세스 없이 끝난 실행(시작 실패 등)이다.
                let current = match p.phase {
                    PostprocessPhase::Pending { run, .. }
                    | PostprocessPhase::Started { run, .. } => Some(run),
                    PostprocessPhase::Finished { .. } => None,
                };
                if current != Some(report.run) {
                    return Err(rejected(
                        task,
                        completion.attempt_id.clone(),
                        CompletionRejection::StaleAttempt,
                    ));
                }
                if let Some(run) = next_run(&spec, report) {
                    // next_run 은 실패 보고에서만 다음 실행을 낸다.
                    if let PostprocessOutcome::Failed { cause, message } = &report.outcome {
                        p.failed_runs.push(PostprocessRunSummary {
                            run: report.run,
                            exit_code: report.exit_code,
                            cause: *cause,
                            message: message.clone(),
                        });
                    }
                    p.phase = PostprocessPhase::Pending {
                        run,
                        not_before_ms: now_ms.saturating_add(spec.retry_delay_ms()),
                    };
                    return self.keep_running(task, p, false);
                }
                if completion.main_copy_dropped {
                    // 기록하지 못한 보고를 대신한 보고다. 레코드의 큰 몫인 본 작업 결과 사본(회차
                    // 진행과 그로 만드는 raw)을 비워야 들어간다.
                    p.execution.output = None;
                }
                let typed = finalize_postprocessed(task, &contract, &spec, &p, report);
                p.phase = PostprocessPhase::Finished { run: report.run };
                if let Some(a) = task.attempt.as_mut() {
                    a.postprocess = Some(p);
                }
                task.result = Some(contract::project_v1(&typed));
                task.typed_result = Some(typed);
                // 접수 응답은 결과의 raw 로 옮겼다.
                task.accepted = None;
                Ok(PostprocessStep::Finalize)
            }
        }
    }

    fn keep_running(
        &mut self,
        task: &mut Task,
        progress: PostprocessProgress,
        duplicate: bool,
    ) -> Result<PostprocessStep> {
        if !duplicate {
            if let Some(a) = task.attempt.as_mut() {
                a.postprocess = Some(progress);
            }
            self.put(task)?;
        }
        Ok(PostprocessStep::Continue(Box::new(CompletionReceipt {
            task: task.clone(),
            transitioned: Vec::new(),
            duplicate,
        })))
    }

    /// 예약한 후처리 실행을 시작한다고 기록한다. 이 쓰기가 끝난 뒤에만 프로세스를 띄운다.
    /// 재시작 때 `Started` 인데 결과가 없으면 같은 실행을 다시 하지 않고 결과 불명으로 끝낸다.
    ///
    /// 반환값은 갱신한 task, 후처리 선언, 본 작업 결과다.
    pub fn begin_postprocess_run(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        attempt_id: &str,
        run: u32,
        now_ms: u64,
    ) -> Result<(Task, PostprocessSpec, TaskResult)> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !matches!(task.state, TaskState::Running) {
            return Err(AgentError::InvalidTransition {
                from: task.state.name().to_string(),
                to: "postprocessing".to_string(),
            });
        }
        let Some(spec) = task.contract.as_ref().and_then(|c| c.postprocess.clone()) else {
            return Err(AgentError::InvalidArgument(format!(
                "task {id} has no postprocess"
            )));
        };
        let attempt = task.attempt.as_mut().filter(|a| a.id == attempt_id);
        let Some(progress) = attempt.and_then(|a| a.postprocess.as_mut()) else {
            return Err(rejected(
                &task,
                Some(attempt_id.to_string()),
                CompletionRejection::StaleAttempt,
            ));
        };
        match progress.phase {
            PostprocessPhase::Pending { run: r, .. } if r == run => {}
            _ => {
                return Err(AgentError::InvalidArgument(format!(
                    "task {id}: postprocess run {run} is not pending"
                )));
            }
        }
        progress.phase = PostprocessPhase::Started {
            run,
            started_at: now_ms,
        };
        let execution = progress.execution.clone();
        self.put(&task)?;
        Ok((task, spec, execution))
    }
}

/// Custom 응답이나 Agent 실행 보고가 값 상한([`MAX_VALUE_BYTES`])을 넘는지. 후처리 진행에 그대로
/// 저장할 수 없는 크기다.
fn response_too_large(task: &Task, result: &TaskResult) -> bool {
    matches!(
        task.command,
        TaskCommand::Custom { .. } | TaskCommand::Agent { .. }
    ) && result
        .output
        .as_ref()
        .is_some_and(|v| serde_json::to_vec(v).map_or(true, |b| b.len() > MAX_VALUE_BYTES))
}
