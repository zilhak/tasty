//! Task primitive — DAG, 상태 머신, memory 영속.
//!
//! 의도적으로 IPC/GUI와 독립적이다. 호스트는 본 모듈의 API를 호출해 task 모델을
//! 영속하고, 별도 스케줄러가 `Ready` 상태 task를 실제로 실행한다 (`ClaudeSpawn`은
//! `claude.spawn` IPC, `Run`은 `tab.create + cmd`, `Custom`은 임의 IPC dispatch,
//! `Reduce`는 본 크레이트 내부 reducer로). 실행 완료 신호가 들어오면 호스트가
//! `TaskStore::set_state`로 진행시키고 downstream의 readiness가 갱신된다.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tasty_utils::id::{SurfaceId, WorkspaceId};

/// Task의 고유 식별자. 형식 `t-<timestamp_ms>-<seq>` (예: `t-1716800000123-7`).
/// 호스트가 본 크레이트 외부에서 임의 문자열로 생성해도 무방하지만, 본 모듈의
/// 헬퍼는 위 형식으로 발급한다.
pub type TaskId = String;

/// Task의 state 머신.
///
/// 변환 규칙:
/// - `Waiting → Ready` (의존성 모두 Succeeded)
/// - `Waiting → Skipped` (의존성 중 하나 Failed, downstream skip 모드)
/// - `Waiting → Cancelled` (사용자/abort)
/// - `Ready → Running` (스케줄러가 실행 시작)
/// - `Ready → Cancelled`
/// - `Running → Succeeded`
/// - `Running → Failed`
/// - `Running → Cancelled`
/// - `Unknown → Ready` (사용자 명시 retry)
/// - `Unknown → Cancelled`
///
/// 재시작 후 `Running` 상태였던 task는 호스트가 `Unknown`으로 표시한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskState {
    Waiting,
    Ready,
    Running,
    Succeeded,
    Failed { error: String },
    Cancelled,
    Skipped,
    Unknown,
}

impl TaskState {
    pub fn name(&self) -> &'static str {
        match self {
            TaskState::Waiting => "waiting",
            TaskState::Ready => "ready",
            TaskState::Running => "running",
            TaskState::Succeeded => "succeeded",
            TaskState::Failed { .. } => "failed",
            TaskState::Cancelled => "cancelled",
            TaskState::Skipped => "skipped",
            TaskState::Unknown => "unknown",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Succeeded
                | TaskState::Failed { .. }
                | TaskState::Cancelled
                | TaskState::Skipped
        )
    }
}

/// Task가 실패했을 때 downstream 처리 정책.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum OnFailure {
    /// downstream 모두 `Skipped`로.
    #[default]
    Abort,
    /// downstream을 정상 진행 (의존성 실패를 성공처럼 취급).
    ContinueDownstream,
    /// Fallback task 를 실행. 그 fallback 이 Succeed 하면 downstream 이 정상 진행.
    ///
    /// 두 형식 지원 (둘 중 정확히 하나):
    /// - `task`: 사전에 존재하는 task id (기존 동작).
    /// - `inline`: 동적 spec — main 이 Failed 가 되는 set_state 분기에서
    ///   `TaskStore::create` 가 새 fallback task 를 생성. `metadata.fallback_of`
    ///   에 main.id 가 자동 주입되어 idempotency 보장 + downstream 재평가 추적.
    Fallback {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<TaskId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline: Option<Box<InlineFallbackSpec>>,
    },
}

/// `OnFailure::Fallback { inline: Some(...) }` 의 동적 생성 spec.
///
/// `depends_on_override` 가 `None` 이면 main task 의 `depends_on` 을 그대로 복사한다.
/// 사용자가 inline 의 `on_failure` 를 또 `Fallback{inline=...}` 으로 nested 하는
/// 경우는 runtime 검증 안 함 — 무한 재귀 방지는 사용자 책임 (max depth 1 권고).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineFallbackSpec {
    pub name: String,
    pub command: TaskCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on_override: Option<Vec<TaskId>>,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Task가 실행할 동작.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskCommand {
    /// `claude.spawn` IPC를 호출해 자식 Claude를 띄운다.
    ClaudeSpawn {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nickname: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_surface: Option<SurfaceId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<String>, // "vertical" | "horizontal"
    },
    /// 새 terminal surface에서 일반 명령 실행.
    Run {
        command: Vec<String>,
        workspace_id: WorkspaceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
    },
    /// 임의 IPC 메서드 호출 위임 (호출자가 해당 메서드의 권한을 보유해야 함).
    Custom {
        ipc_method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// 다른 task의 결과를 합성.
    Reduce {
        inputs: Vec<TaskId>,
        strategy: ReducerStrategy,
    },
    /// 명시적 barrier 대기. barrier 가 Closed 되면 Succeeded, TimedOut 이면 Failed.
    /// timeout 은 barrier 자체의 `timeout_ms` 가 단일 출처.
    WaitBarrier { name: String },
}

/// Reducer 합성 전략.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReducerStrategy {
    FirstSuccess,
    All,
    MergeJson,
    ConcatText,
    /// shell 명령으로 결과 배열 stdin 전달, stdout이 최종.
    Custom {
        command: String,
    },
}

/// Task 실행 결과. `Succeeded`/`Failed` 상태에서만 채워진다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskResult {
    /// 종료 코드 (Run의 exit_code, ClaudeSpawn의 wait 결과 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 실행 산출물 (claude의 surface id, Custom의 IPC 응답 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    /// 에러 사유 (Failed 상태에서만).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 영속되는 Task 한 레코드.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub command: TaskCommand,
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
    pub state: TaskState,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<TaskResult>,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// 워크스페이스에 속한 task들의 그래프 뷰. 사이클 검출, downstream 계산용.
pub(super) const TASK_KEY_PREFIX: &str = "tasty.agent.task.";

pub(super) fn task_key(id: &TaskId) -> String {
    format!("{TASK_KEY_PREFIX}{id}")
}

/// `MemoryStore` 위에 얹은 Task 영속 + state 머신.
///
/// 본 store는 빌려 쓰는 형태. 호스트가 `MemoryStore`의 lock을 잡은 상태에서
/// 임시로 wrap해 호출한다.
pub(super) fn is_valid_transition(from: &TaskState, to: &TaskState) -> bool {
    use TaskState::*;
    match (from, to) {
        (Waiting, Ready)
        | (Waiting, Skipped)
        | (Waiting, Cancelled)
        | (Ready, Running)
        | (Ready, Cancelled)
        | (Running, Succeeded)
        | (Running, Failed { .. })
        | (Running, Cancelled)
        | (Unknown, Ready)
        | (Unknown, Cancelled)
        | (Unknown, Waiting) => true,
        // retry 경로는 별도 메서드에서 처리. 직접 set_state로는 거부.
        _ => false,
    }
}

/// on_failure 정책에 따라 downstream task의 목표 상태를 결정.
/// 호출 시점 `task`는 dep 중 하나가 실패해 `evaluate_readiness`가 `Skipped`를
/// 반환한 상태로 가정한다.
pub(super) fn apply_on_failure(task: &Task, _all: &[Task]) -> Option<TaskState> {
    match &task.on_failure {
        OnFailure::Abort => Some(TaskState::Skipped),
        OnFailure::ContinueDownstream => Some(TaskState::Ready),
        // Fallback은 호스트가 fallback task를 별도 트리거해야 함. 본 task는 일단
        // Waiting을 유지하고, fallback이 Succeed하면 그때 호스트가 다시 평가.
        OnFailure::Fallback { .. } => None,
    }
}

mod graph;
mod store;

pub use graph::*;
pub use store::*;

#[cfg(test)]
#[path = "task/tests.rs"]
mod tests;
