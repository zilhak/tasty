//! v2 task 의 후처리 슬롯 — 본 작업 뒤 CLI 하나를 실행해 stdout 을 최종 출력으로 수집한다.
//!
//! 이 모듈은 계약(선언·검증), stdin 에 쓸 JSON 문서 조립, stdout 해석, 실행 보고 형식만 다룬다.
//! 프로세스 실행·시간 제한·취소는 호스트 실행기가 맡는다. 모델 접속·인증·질문 작성은 호출하는
//! CLI 의 책임이며 Tasty 는 명령 실행·입출력·타입 검증·실패 처리만 한다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::binding::pointer_tokens;
use super::contract::{
    FailureStage, Provenance, RawResult, TaskContract, TaskFailure, TypedResult, command_kind,
};
use super::types::TypedValue;
use super::{Task, TaskResult};

/// `timeout_ms` 상한(24시간). 유한한 제한만 받는다.
pub const MAX_POSTPROCESS_TIMEOUT_MS: u64 = 86_400_000;
/// 자동 재시도 횟수 상한.
pub const MAX_POSTPROCESS_RETRIES: u32 = 10;
/// 재시도 대기 상한(1시간).
pub const MAX_POSTPROCESS_RETRY_DELAY_MS: u64 = 3_600_000;
/// stdout 수집 상한. 넘으면 잘린 값으로 성공시키지 않고 실패한다. 수집한 값은 task 레코드에
/// 저장되며 레코드 하나는 memory 값 상한(1 MiB) 안에 들어야 한다.
pub const MAX_POSTPROCESS_STDOUT_BYTES: usize = 256 * 1024;
/// stderr 는 진단용으로 마지막 부분만 남긴다.
pub const POSTPROCESS_STDERR_TAIL_BYTES: usize = 16 * 1024;

/// 후처리 선언.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostprocessSpec {
    /// 실행 파일과 인자. 직접 실행하며 셸을 거치지 않는다. 셸이 필요하면 셸 실행 파일을 명시한다.
    pub command: Vec<String>,
    /// 작업 디렉터리. 생략하면 run 의 `cwd`, 그것도 없으면 호스트 프로세스의 디렉터리다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// stdin 에 쓸 JSON object 의 필드별 출처. 비어 있으면 `{}` 를 쓴다.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stdin: BTreeMap<String, PostprocessSource>,
    #[serde(default)]
    pub stdout: StdoutSpec,
    /// 실행 제한. 프로세스 종료와 stdin 쓰기·출력 파이프 EOF 까지 포함한다.
    pub timeout_ms: u64,
    /// 자동 재시도. 생략하면 재시도하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<PostprocessRetry>,
}

/// stdin 필드 하나의 출처.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostprocessSource {
    pub from: SourceDocument,
    /// 출처 문서 안의 RFC 6901 위치. 생략하면 문서 전체다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}

/// 후처리가 읽을 수 있는 회차 자료.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDocument {
    /// 이 회차의 입력 snapshot 값(wire 형식). 입력이 unit 이면 null.
    Input,
    /// 본 작업의 원본 결과 `{exit_code?, execution?}`.
    Raw,
    /// 없어진 출처 `artifacts`. 저장된 레코드를 읽을 수 있게 해석만 하고, 제출은 거절하며
    /// 저장된 task 에서는 빈 배열이다.
    #[serde(rename = "artifacts")]
    RemovedArtifacts,
}

/// stdout 수집 방식.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdoutSpec {
    #[serde(default)]
    pub format: StdoutFormat,
    /// json 형식에서 최종 출력 후보를 고르는 RFC 6901 위치. 생략하면 값 전체다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdoutFormat {
    /// JSON 값 하나. 파싱에 실패해도 text 로 바꾸지 않는다.
    #[default]
    Json,
    /// UTF-8 문자열 그대로.
    Text,
}

/// 자동 재시도 정책. 재시도는 저장한 본 작업 raw 로 후처리만 다시 실행한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostprocessRetry {
    /// 첫 실행 뒤 더 실행할 수 있는 횟수.
    pub max_retries: u32,
    /// 실패 뒤 다음 실행까지 기다리는 시간.
    #[serde(default)]
    pub delay_ms: u64,
}

impl PostprocessSpec {
    /// 이 회차에서 실행할 수 있는 최대 횟수(첫 실행 포함).
    pub fn max_runs(&self) -> u32 {
        1 + self.retry.as_ref().map_or(0, |r| r.max_retries)
    }

    pub fn retry_delay_ms(&self) -> u64 {
        self.retry.as_ref().map_or(0, |r| r.delay_ms)
    }
}

/// 생성 시 선언을 검사한다. 오류 위치는 task 기준 상대 위치(`/postprocess/...`)다.
pub fn check_spec(spec: &PostprocessSpec) -> Result<(), TaskFailure> {
    let err = |loc: &str, message: String| {
        let mut f = TaskFailure::new(FailureStage::Input, message);
        f.location = Some(format!("/postprocess{loc}"));
        f
    };
    if spec.command.is_empty() || spec.command[0].is_empty() {
        return Err(err(
            "/command",
            "postprocess command needs an executable".into(),
        ));
    }
    if spec.timeout_ms == 0 || spec.timeout_ms > MAX_POSTPROCESS_TIMEOUT_MS {
        return Err(err(
            "/timeout_ms",
            format!("postprocess timeout_ms must be 1..={MAX_POSTPROCESS_TIMEOUT_MS}"),
        ));
    }
    if let Some(r) = &spec.retry {
        if r.max_retries == 0 || r.max_retries > MAX_POSTPROCESS_RETRIES {
            return Err(err(
                "/retry/max_retries",
                format!("postprocess retry max_retries must be 1..={MAX_POSTPROCESS_RETRIES}"),
            ));
        }
        if r.delay_ms > MAX_POSTPROCESS_RETRY_DELAY_MS {
            return Err(err(
                "/retry/delay_ms",
                format!(
                    "postprocess retry delay_ms must be at most {MAX_POSTPROCESS_RETRY_DELAY_MS}"
                ),
            ));
        }
    }
    for (field, source) in &spec.stdin {
        if source.from == SourceDocument::RemovedArtifacts {
            return Err(err(
                &format!("/stdin/{field}/from"),
                "the artifacts source was removed".to_string(),
            ));
        }
        if let Some(p) = &source.pointer {
            pointer_tokens(p).map_err(|m| err(&format!("/stdin/{field}/pointer"), m))?;
        }
    }
    match (&spec.stdout.format, &spec.stdout.pointer) {
        (StdoutFormat::Text, Some(_)) => {
            return Err(err(
                "/stdout/pointer",
                "stdout pointer applies to the json format only".into(),
            ));
        }
        (StdoutFormat::Json, Some(p)) => {
            pointer_tokens(p).map_err(|m| err("/stdout/pointer", m))?;
        }
        _ => {}
    }
    Ok(())
}

// ── 실행 보고 ────────────────────────────────────────────────────────────────

/// 후처리 실행 한 번의 보고. 호스트 실행기가 만들고 저장소가 결과를 확정한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostprocessReport {
    /// 회차 안의 실행 번호(1부터).
    pub run: u32,
    /// 프로세스 종료 코드. 시작하지 못했거나 신호로 끝났으면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// stderr 의 마지막 부분. 진단용이며 출력에 섞지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// stderr 를 앞부분에서 잘랐는가.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stderr_truncated: bool,
    pub outcome: PostprocessOutcome,
}

/// 실행 결과. `Collected` 의 `stdout` 은 해석한 stdout 전체(json 값 또는 text 문자열)이며
/// pointer 가 있으면 그 위치에 값이 있음을 확인했다. 출력 검증은 결과 확정 때 한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PostprocessOutcome {
    Collected {
        stdout: Value,
    },
    Failed {
        cause: PostprocessCause,
        message: String,
    },
}

/// 후처리 실패 원인.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostprocessCause {
    /// stdin 출처가 가리키는 값이 없다.
    StdinMapping,
    /// 실행 파일을 시작하지 못했다.
    Spawn,
    /// stdin 을 쓰지 못했다(읽지 않고 닫은 경우는 제외).
    StdinWrite,
    /// 0 이 아닌 종료 코드.
    NonzeroExit,
    /// 숫자 종료 코드 없이 끝났다(신호 등).
    Signal,
    /// 제한 시간 안에 종료·출력 수집이 끝나지 않았다.
    Timeout,
    /// task 취소로 중단했다.
    Cancelled,
    /// stdout 이 수집 상한을 넘었다.
    StdoutTooLarge,
    /// stdout·stderr 를 읽지 못했다.
    StdoutRead,
    /// stdout 이 UTF-8 이 아니다.
    InvalidUtf8,
    /// json 형식인데 JSON 이 아니다.
    InvalidJson,
    /// json 형식인데 JSON 값이 둘 이상이다.
    MultipleDocuments,
    /// json 형식인데 stdout 이 비었다.
    EmptyOutput,
    /// stdout pointer 가 가리키는 값이 없다.
    PointerMissing,
    /// 실행이 시작됐지만 호스트가 결과를 받기 전에 끝났다. 자동으로 다시 실행하지 않는다.
    OutcomeUnknown,
}

impl PostprocessCause {
    /// 자동 재시도 대상인가. 취소·결과 불명·입력 매핑 오류는 다시 실행해도 의미가 없거나
    /// (결과 불명) 중복 실행을 만들 수 있어 재시도하지 않는다.
    pub fn retryable(self) -> bool {
        !matches!(
            self,
            PostprocessCause::StdinMapping
                | PostprocessCause::Cancelled
                | PostprocessCause::OutcomeUnknown
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            PostprocessCause::StdinMapping => "stdin_mapping",
            PostprocessCause::Spawn => "spawn",
            PostprocessCause::StdinWrite => "stdin_write",
            PostprocessCause::NonzeroExit => "nonzero_exit",
            PostprocessCause::Signal => "signal",
            PostprocessCause::Timeout => "timeout",
            PostprocessCause::Cancelled => "cancelled",
            PostprocessCause::StdoutTooLarge => "stdout_too_large",
            PostprocessCause::StdoutRead => "stdout_read",
            PostprocessCause::InvalidUtf8 => "invalid_utf8",
            PostprocessCause::InvalidJson => "invalid_json",
            PostprocessCause::MultipleDocuments => "multiple_documents",
            PostprocessCause::EmptyOutput => "empty_output",
            PostprocessCause::PointerMissing => "pointer_missing",
            PostprocessCause::OutcomeUnknown => "outcome_unknown",
        }
    }
}

impl PostprocessReport {
    /// 프로세스 없이 끝난 실패 보고.
    pub fn failed(run: u32, cause: PostprocessCause, message: impl Into<String>) -> Self {
        Self {
            run,
            exit_code: None,
            stderr: None,
            stderr_truncated: false,
            outcome: PostprocessOutcome::Failed {
                cause,
                message: message.into(),
            },
        }
    }

    /// 실패 원인. 수집에 성공했으면 없다.
    pub fn cause(&self) -> Option<PostprocessCause> {
        match &self.outcome {
            PostprocessOutcome::Failed { cause, .. } => Some(*cause),
            PostprocessOutcome::Collected { .. } => None,
        }
    }
}

// ── stdin 문서 ──────────────────────────────────────────────────────────────

/// 회차 자료에서 stdin 에 쓸 JSON object 를 만든다. `execution` 은 저장한 본 작업 결과다.
pub fn stdin_document(
    task: &Task,
    spec: &PostprocessSpec,
    execution: &TaskResult,
) -> Result<Value, (PostprocessCause, String)> {
    let input = task
        .input_snapshot
        .as_ref()
        .map(|s| s.value.to_wire())
        .unwrap_or(Value::Null);
    let mut raw = serde_json::Map::new();
    if let Some(code) = execution.exit_code {
        raw.insert("exit_code".into(), Value::from(code));
    }
    if let Some(v) = &execution.output {
        raw.insert("execution".into(), v.clone());
    }
    if let Some(a) = &task.accepted
        && let Ok(v) = serde_json::to_value(a)
    {
        raw.insert("accepted".into(), v);
    }
    let raw = Value::Object(raw);
    let removed_artifacts = Value::Array(Vec::new());
    let mut doc = serde_json::Map::new();
    for (field, source) in &spec.stdin {
        let base = match source.from {
            SourceDocument::Input => &input,
            SourceDocument::Raw => &raw,
            SourceDocument::RemovedArtifacts => &removed_artifacts,
        };
        let value = match &source.pointer {
            None => base.clone(),
            Some(p) => base.pointer(p).cloned().ok_or_else(|| {
                (
                    PostprocessCause::StdinMapping,
                    format!("stdin field '{field}': pointer '{p}' has no value"),
                )
            })?,
        };
        doc.insert(field.clone(), value);
    }
    Ok(Value::Object(doc))
}

// ── stdout 해석 ─────────────────────────────────────────────────────────────

/// 수집한 stdout 을 해석한다. `overflowed` 는 수집 상한을 넘겼다는 뜻이다. 돌려주는 값은
/// stdout 전체이며 pointer 위치는 [`output_candidate`] 로 고른다.
pub fn collect_stdout(
    stdout: &StdoutSpec,
    bytes: &[u8],
    overflowed: bool,
) -> Result<Value, (PostprocessCause, String)> {
    if overflowed {
        return Err((
            PostprocessCause::StdoutTooLarge,
            format!("stdout exceeded {MAX_POSTPROCESS_STDOUT_BYTES} bytes"),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        (
            PostprocessCause::InvalidUtf8,
            format!("stdout is not UTF-8: {e}"),
        )
    })?;
    match stdout.format {
        StdoutFormat::Text => Ok(Value::String(text.to_string())),
        StdoutFormat::Json => {
            if text.trim().is_empty() {
                return Err((PostprocessCause::EmptyOutput, "stdout is empty".into()));
            }
            let mut values = serde_json::Deserializer::from_str(text).into_iter::<Value>();
            let value = match values.next() {
                Some(Ok(v)) => v,
                Some(Err(e)) => {
                    return Err((
                        PostprocessCause::InvalidJson,
                        format!("stdout is not JSON: {e}"),
                    ));
                }
                None => return Err((PostprocessCause::EmptyOutput, "stdout is empty".into())),
            };
            match values.next() {
                None => {}
                Some(Ok(_)) => {
                    return Err((
                        PostprocessCause::MultipleDocuments,
                        "stdout holds more than one JSON value".into(),
                    ));
                }
                Some(Err(e)) => {
                    return Err((
                        PostprocessCause::InvalidJson,
                        format!("stdout has trailing text after the JSON value: {e}"),
                    ));
                }
            }
            if let Some(p) = &stdout.pointer
                && value.pointer(p).is_none()
            {
                return Err((
                    PostprocessCause::PointerMissing,
                    format!("stdout pointer '{p}' has no value"),
                ));
            }
            Ok(value)
        }
    }
}

/// 해석한 stdout 에서 최종 출력 후보를 고른다. pointer 가 없으면 전체다.
pub fn output_candidate(stdout: &StdoutSpec, document: &Value) -> Option<Value> {
    match &stdout.pointer {
        None => Some(document.clone()),
        Some(p) => document.pointer(p).cloned(),
    }
}

// ── 회차 진행 ───────────────────────────────────────────────────────────────

/// 본 작업이 성공한 뒤의 후처리 진행. 회차([`super::TaskAttempt`])에 저장한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostprocessProgress {
    /// 본 작업의 원본 결과. 재시도는 이 값으로 후처리만 다시 실행한다.
    pub execution: TaskResult,
    /// 본 작업 보고의 지문. 같은 보고의 재전송을 알아본다.
    pub main_digest: String,
    pub phase: PostprocessPhase,
    /// 앞서 실패하고 재시도로 넘어간 실행들.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_runs: Vec<PostprocessRunSummary>,
}

/// 후처리 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PostprocessPhase {
    /// `run` 번째 실행을 `not_before_ms` 이후 시작한다. 2번째 실행부터는 재시도 대기다.
    Pending { run: u32, not_before_ms: u64 },
    /// `run` 번째 실행을 시작했다. 결과 없이 호스트가 재시작하면 결과 불명으로 끝낸다.
    Started { run: u32, started_at: u64 },
    /// 더 실행하지 않는다. `run` 번째 실행의 보고로 회차를 확정했거나, 그 전에 task 가
    /// 취소 등으로 끝났다. 뒤의 경우 `run` 은 마지막으로 시작한 실행 번호다(없으면 0).
    Finished { run: u32 },
}

/// 완료 보고 밖에서(취소 등) 종결된 task 의 후처리 단계를 닫는다. 기록을 읽는 쪽이 끝난
/// task 를 진행 중으로 보지 않게 한다.
pub(crate) fn close_phase(task: &mut Task) {
    let Some(progress) = task.attempt.as_mut().and_then(|a| a.postprocess.as_mut()) else {
        return;
    };
    let run = match progress.phase {
        PostprocessPhase::Pending { run, .. } => run - 1,
        PostprocessPhase::Started { run, .. } => run,
        PostprocessPhase::Finished { .. } => return,
    };
    progress.phase = PostprocessPhase::Finished { run };
}

/// 재시도로 넘어간 실행 하나의 요약.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostprocessRunSummary {
    pub run: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub cause: PostprocessCause,
    pub message: String,
}

impl PostprocessProgress {
    /// task 조회에 보이는 진행 단계 이름.
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            PostprocessPhase::Pending { run, .. } if run > 1 => "retry_wait",
            _ => "postprocessing",
        }
    }
}

impl Task {
    /// Running 인 v2 task 가 후처리 단계에 있으면 그 이름(`postprocessing`·`retry_wait`).
    pub fn postprocess_phase(&self) -> Option<&'static str> {
        if !matches!(self.state, super::TaskState::Running) {
            return None;
        }
        let progress = self.attempt.as_ref()?.postprocess.as_ref()?;
        (!matches!(progress.phase, PostprocessPhase::Finished { .. }))
            .then(|| progress.phase_name())
    }
}

/// 이 보고 뒤에 이어 실행할 번호. 없으면 이 보고로 회차를 확정한다.
pub fn next_run(spec: &PostprocessSpec, report: &PostprocessReport) -> Option<u32> {
    let cause = report.cause()?;
    (cause.retryable() && report.run < spec.max_runs()).then_some(report.run + 1)
}

/// 후처리 원본 결과. 본 작업의 원본(`raw.exit_code`·`raw.execution`)과 따로 둔다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostprocessRaw {
    /// 실행한 명령. 보고 당시의 계약 값이다.
    pub command: Vec<String>,
    /// 회차 안의 실행 번호.
    pub run: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// pointer 로 일부를 골랐을 때의 stdout 전체. pointer 가 없으면 출력과 같아 싣지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stderr_truncated: bool,
    /// 실패 원인. 성공한 실행이면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<PostprocessCause>,
    /// 재시도로 넘어간 앞선 실행들.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_runs: Vec<PostprocessRunSummary>,
}

/// 마지막 후처리 보고로 v2 결과를 확정한다. 출력 후보가 출력 타입에 맞지 않으면
/// `output_validation` 실패, 후처리가 실패했으면 `postprocess` 실패다.
pub fn finalize_postprocessed(
    task: &Task,
    contract: &TaskContract,
    spec: &PostprocessSpec,
    progress: &PostprocessProgress,
    report: &PostprocessReport,
) -> TypedResult {
    let mut raw = RawResult {
        exit_code: progress.execution.exit_code,
        execution: progress.execution.output.clone(),
        postprocess: Some(PostprocessRaw {
            command: spec.command.clone(),
            run: report.run,
            exit_code: report.exit_code,
            stdout: None,
            stderr: report.stderr.clone(),
            stderr_truncated: report.stderr_truncated,
            cause: report.cause(),
            failed_runs: progress.failed_runs.clone(),
        }),
        accepted: task.accepted.clone(),
    };
    let provenance = |source: &str| Provenance {
        contract_version: contract.contract_version,
        kind: command_kind(&task.command).to_string(),
        output_source: source.to_string(),
    };
    let failed = |raw: RawResult, failure: TaskFailure, source: &str| TypedResult {
        has_output: false,
        output: TypedValue::Null,
        raw,
        error: Some(failure),
        provenance: provenance(source),
    };
    let stdout = match &report.outcome {
        PostprocessOutcome::Failed { cause, message } => {
            let failure = TaskFailure::new(
                FailureStage::Postprocess,
                format!("postprocess {}: {message}", cause.name()),
            );
            return failed(raw, failure, "none");
        }
        PostprocessOutcome::Collected { stdout } => stdout,
    };
    let source = match spec.stdout.format {
        StdoutFormat::Json => "postprocess.stdout.json",
        StdoutFormat::Text => "postprocess.stdout.text",
    };
    if spec.stdout.pointer.is_some()
        && let Some(pp) = raw.postprocess.as_mut()
    {
        pp.stdout = Some(stdout.clone());
    }
    let Some(candidate) = output_candidate(&spec.stdout, stdout) else {
        let failure = TaskFailure::new(
            FailureStage::Postprocess,
            "postprocess pointer_missing: stdout pointer has no value",
        );
        return failed(raw, failure, source);
    };
    let schema = contract.output_schema(&task.command);
    match contract.defs().validate_typed(&schema, &candidate) {
        Ok(output) => TypedResult {
            has_output: true,
            output,
            raw,
            error: None,
            provenance: provenance(source),
        },
        Err(e) => failed(
            raw,
            TaskFailure::typed(FailureStage::OutputValidation, Some(task.id.clone()), e),
            source,
        ),
    }
}

#[cfg(test)]
#[path = "postprocess_tests.rs"]
mod tests;
