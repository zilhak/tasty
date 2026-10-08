//! DAG report. task 회차마다 블록 하나를 둔다.
//!
//! 블록은 auto 와 custom 으로 이뤄진다. auto 는 저장하지 않고 조회 때 task 레코드(회차·입력
//! snapshot·결과·실패·skip)에서 투영한다([`project_task`]). custom 은 회차가 실행되는 동안
//! 실행 중인 프로그램이 텍스트로 더하는 기록이며, 회차마다 memory 키 하나([`report_key`])에
//! 저장한다. task 레코드와 키를 나눠 report 크기가 task 상태 전이의 저장을 막지 않게 한다.
//!
//! append 는 task 레코드가 그 회차를 아직 열어 두었을 때(Ready·Running 이고 토큰이 같을 때)만
//! 받는다. 종결 전이는 task 레코드를 쓰는 한 번의 저장이고 append 도 같은 저장소 락 안에서
//! 레코드를 읽고 판정하므로, 블록은 종결 전이를 저장한 시점의 내용으로 고정된다.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Task, TaskCommand, TaskId, TaskState};

/// custom 블록을 저장하는 memory 키 접두. 뒤에 `<task id>.<회차 번호>` 가 온다.
pub const REPORT_KEY_PREFIX: &str = "tasty.agent.task_report.";

/// 실행 중인 자식에게 자기 블록을 알려 주는 환경 변수.
pub const REPORT_ENV: &str = "TASTY_TASK_REPORT";

/// run stderr 에서 이 접두로 시작하는 줄은 report 에 append 한다.
pub const STDERR_MARKER: &str = "::tasty-report::";

/// append 한 번의 기본 상한(바이트).
pub const DEFAULT_APPEND_LIMIT: u64 = 1024;
/// 블록 하나의 기본 상한(바이트, 저장한 텍스트 합).
pub const DEFAULT_BLOCK_LIMIT: u64 = 16 * 1024;
/// append 상한으로 받는 범위.
pub const APPEND_LIMIT_RANGE: (u64, u64) = (64, 64 * 1024);
/// 블록 상한으로 받는 범위. 위쪽은 JSON 이스케이프로 텍스트가 여러 배 커져도 memory 값 하나의
/// 상한(1 MiB) 안에 들도록 잡았다.
pub const BLOCK_LIMIT_RANGE: (u64, u64) = (128, 128 * 1024);

/// report 크기 상한. 항상 `append_bytes < block_bytes` 다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportLimits {
    pub append_bytes: u64,
    pub block_bytes: u64,
}

impl Default for ReportLimits {
    fn default() -> Self {
        Self {
            append_bytes: DEFAULT_APPEND_LIMIT,
            block_bytes: DEFAULT_BLOCK_LIMIT,
        }
    }
}

impl ReportLimits {
    /// 범위와 `append < block` 을 확인한다. 어긋나면 이유를 돌려준다.
    pub fn validate(&self) -> Result<(), String> {
        let in_range = |v: u64, (lo, hi): (u64, u64), name: &str| {
            if v < lo || v > hi {
                Err(format!("{name} {v} is outside {lo}..={hi}"))
            } else {
                Ok(())
            }
        };
        in_range(self.append_bytes, APPEND_LIMIT_RANGE, "append limit")?;
        in_range(self.block_bytes, BLOCK_LIMIT_RANGE, "block limit")?;
        if self.append_bytes >= self.block_bytes {
            return Err(format!(
                "append limit {} must be smaller than block limit {}",
                self.append_bytes, self.block_bytes
            ));
        }
        Ok(())
    }
}

/// custom 기록을 낸 경로. 호출자가 밝힌 값이며 인증하지는 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportSource {
    Run,
    Postprocess,
    Agent,
    ReduceCustom,
    StderrMarker,
}

impl ReportSource {
    pub fn name(self) -> &'static str {
        match self {
            ReportSource::Run => "run",
            ReportSource::Postprocess => "postprocess",
            ReportSource::Agent => "agent",
            ReportSource::ReduceCustom => "reduce_custom",
            ReportSource::StderrMarker => "stderr_marker",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ReportSource::Run,
            ReportSource::Postprocess,
            ReportSource::Agent,
            ReportSource::ReduceCustom,
            ReportSource::StderrMarker,
        ]
        .into_iter()
        .find(|v| v.name() == s)
    }
}

/// dispatch 가 발급한 회차의 report 토큰. 다음 dispatch 가 새 값으로 바꾼다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportToken {
    /// 이 토큰이 여는 회차 번호.
    pub attempt: u32,
    pub token: String,
}

/// custom 기록 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportEntry {
    /// 블록 안의 append 순번(1부터). 저장하지 못한 append 도 번호를 쓴다.
    pub seq: u64,
    pub at: u64,
    pub source: ReportSource,
    pub text: String,
    /// append 상한을 넘어 잘린 바이트 수. 잘리지 않았으면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omit_by_limit: Option<u64>,
}

/// 회차 하나의 custom 블록.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReportBlock {
    pub attempt: u32,
    #[serde(default)]
    pub entries: Vec<ReportEntry>,
    /// 블록 상한을 넘어 저장하지 않은 append 수.
    #[serde(default)]
    pub omitted_appends: u64,
    /// 저장한 텍스트의 바이트 합. 블록 상한과 비교한다.
    #[serde(default)]
    pub stored_bytes: u64,
    /// 뒤 회차가 시작될 때(retry) 이 회차가 끝난 상태. 마지막 회차는 task 의 상태를 쓴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled: Option<TaskState>,
}

/// append 결과. 블록 상한에 걸려도 실패가 아니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum AppendOutcome {
    Stored {
        seq: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        omit_by_limit: Option<u64>,
    },
    /// 블록 상한을 넘어 저장하지 않았다.
    Omitted { seq: u64 },
}

impl ReportBlock {
    pub fn new(attempt: u32) -> Self {
        Self {
            attempt,
            ..Self::default()
        }
    }

    /// 텍스트 하나를 상한에 맞춰 더한다.
    pub fn append(
        &mut self,
        source: ReportSource,
        text: &str,
        limits: ReportLimits,
        now_ms: u64,
    ) -> AppendOutcome {
        let seq = self.entries.len() as u64 + self.omitted_appends + 1;
        let (text, omit_by_limit) = truncate_text(text, limits.append_bytes as usize);
        if self.stored_bytes + text.len() as u64 > limits.block_bytes {
            self.omitted_appends += 1;
            return AppendOutcome::Omitted { seq };
        }
        self.stored_bytes += text.len() as u64;
        self.entries.push(ReportEntry {
            seq,
            at: now_ms,
            source,
            text,
            omit_by_limit,
        });
        AppendOutcome::Stored { seq, omit_by_limit }
    }
}

/// `limit` 바이트를 넘으면 UTF-8 경계에서 잘라 남긴 텍스트와 잘린 바이트 수를 돌려준다.
pub fn truncate_text(text: &str, limit: usize) -> (String, Option<u64>) {
    if text.len() <= limit {
        return (text.to_string(), None);
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_string(), Some((text.len() - cut) as u64))
}

/// 회차 블록의 memory 키. 회차 번호에 `.` 이 없어 task id 가 달라도 키가 겹치지 않는다.
pub fn report_key(task_id: &TaskId, attempt: u32) -> crate::Result<String> {
    crate::component_key(
        REPORT_KEY_PREFIX,
        "task id",
        &format!("{task_id}.{attempt}"),
    )
}

/// append 가 찾아갈 블록. [`REPORT_ENV`] 값의 형식은 `<workspace>/<회차>/<source>/<토큰>/<task id>` 다.
/// task id 를 맨 뒤에 두어 그 안의 문자와 구분자가 섞이지 않게 한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportAddress {
    pub workspace_id: u32,
    pub task_id: TaskId,
    pub attempt: u32,
    pub source: ReportSource,
    pub token: String,
}

impl ReportAddress {
    pub fn to_env_value(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            self.workspace_id,
            self.attempt,
            self.source.name(),
            self.token,
            self.task_id
        )
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let bad = || {
            format!("{REPORT_ENV} value is not <workspace>/<attempt>/<source>/<token>/<task id>")
        };
        let mut parts = value.trim().splitn(5, '/');
        let mut next = || parts.next().filter(|s| !s.is_empty()).ok_or_else(bad);
        let workspace_id = next()?.parse().map_err(|_| bad())?;
        let attempt = next()?.parse().map_err(|_| bad())?;
        let source = ReportSource::parse(next()?).ok_or_else(bad)?;
        let token = next()?.to_string();
        let task_id = next()?.to_string();
        Ok(Self {
            workspace_id,
            task_id,
            attempt,
            source,
            token,
        })
    }

    /// 같은 블록을 다른 경로로 쓰는 주소.
    pub fn with_source(&self, source: ReportSource) -> Self {
        Self {
            source,
            ..self.clone()
        }
    }
}

/// task 하나의 report. `attempt` 를 주면 그 회차의 블록만 싣고, auto 는 그 회차가 마지막
/// 회차일 때만 싣는다(이전 회차의 결과는 retry 가 지워 투영할 수 없다).
pub fn project_task(
    task: &Task,
    blocks: &[ReportBlock],
    attempt: Option<u32>,
    include_raw: bool,
) -> Value {
    let current = current_attempt(task);
    let auto = match attempt {
        Some(n) if Some(n) != current => Value::Null,
        _ => project_auto(task, include_raw),
    };
    let mut numbers: Vec<u32> = blocks.iter().map(|b| b.attempt).collect();
    if let Some(n) = current
        && !matches!(task.state, TaskState::Skipped)
    {
        numbers.push(n);
    }
    numbers.sort_unstable();
    numbers.dedup();
    let attempts: Vec<Value> = numbers
        .into_iter()
        .filter(|n| attempt.is_none_or(|a| a == *n))
        .map(|n| {
            let block = blocks.iter().find(|b| b.attempt == n);
            let state = if Some(n) == current {
                Some(task.state.clone())
            } else {
                block.and_then(|b| b.settled.clone())
            };
            json!({
                "attempt": n,
                "state": state,
                "custom": block.map_or_else(|| json!({"entries": [], "omitted_appends": 0}), |b| json!({
                    "entries": b.entries,
                    "omitted_appends": b.omitted_appends,
                })),
            })
        })
        .collect();
    json!({
        "task_id": task.id,
        "name": task.name,
        "auto": auto,
        "attempts": attempts,
    })
}

/// 지금 task 가 가리키는 회차 번호. 실행 중인 회차, 끝난 마지막 회차, 아직 Running 으로
/// 넘어가지 않은 dispatch 의 회차 중 가장 큰 값이다.
pub fn current_attempt(task: &Task) -> Option<u32> {
    let ran = task.attempt.as_ref().map(|a| a.number);
    let issued = task.report_token.as_ref().map(|t| t.attempt);
    ran.max(issued)
}

/// 저장 키를 지워야 할 회차의 최대 번호.
pub fn max_attempt(task: &Task) -> u32 {
    current_attempt(task).unwrap_or(0)
}

fn project_auto(task: &Task, include_raw: bool) -> Value {
    let typed = task.typed_result.as_ref();
    let mut auto = json!({
        "kind": super::contract::command_kind(&task.command),
        "state": task.state,
        "started_at": task.started_at,
        "finished_at": task.finished_at,
        "input": task.input_snapshot.as_ref().map(|s| s.value.to_wire()),
        "output": match typed {
            Some(t) if t.has_output => t.output.to_wire(),
            Some(_) => Value::Null,
            None => task.result.as_ref().and_then(|r| r.output.clone()).unwrap_or(Value::Null),
        },
        "failure": typed.and_then(|t| t.error.as_ref()).map(|e| json!({
            "stage": e.stage,
            "location": e.location,
            "message": e.message,
        })),
        "skip": task.skip,
    });
    let exit_code = typed
        .and_then(|t| t.raw.exit_code)
        .or_else(|| task.result.as_ref().and_then(|r| r.exit_code));
    let detail = match &task.command {
        TaskCommand::Run { .. } => json!({ "exit_code": exit_code }),
        TaskCommand::Agent { provider, .. } => {
            let link = task.attempt.as_ref().and_then(|a| a.agent.as_ref());
            json!({
                "provider": link.map_or(provider.as_str(), |l| l.provider.as_str()),
                "surface_id": link.map(|l| l.surface_id),
            })
        }
        TaskCommand::Custom { ipc_method, .. } => json!({ "ipc_method": ipc_method }),
        TaskCommand::Reduce { inputs, strategy } => json!({
            "strategy": serde_json::to_value(strategy).ok().and_then(|v| v.get("kind").cloned()),
            "input_count": inputs.len(),
        }),
        TaskCommand::WaitBarrier { .. } => json!({ "barrier": task.barrier_name() }),
    };
    if let (Some(map), Value::Object(extra)) = (auto.as_object_mut(), detail) {
        map.extend(extra);
        if include_raw {
            let execution = typed
                .and_then(|t| t.raw.execution.as_ref())
                .or_else(|| task.result.as_ref().and_then(|r| r.output.as_ref()));
            let stream = |name: &str| execution.and_then(|e| e.get(name)).cloned();
            map.insert(
                "raw".into(),
                json!({ "stdout": stream("stdout"), "stderr": stream("stderr") }),
            );
        }
    }
    auto
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
