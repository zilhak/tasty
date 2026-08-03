//! Tasty Agent — 다중 에이전트 협업 primitive.
//!
//! 1차 시민으로 다음을 제공:
//! - **Task DAG**: 의존성을 가진 task 그래프. state 머신과 사이클 검출.
//! - (후속) Barrier / Semaphore / Lease / Reducer / Rate Limit.
//!
//! 영속은 `tasty-memory` 위에 얹는다 (scope = workspace, key prefix =
//! `tasty.agent.task.<id>`). 본 크레이트는 GUI/IPC와 독립적이며 순수 상태
//! 머신 + 영속 헬퍼만 담당한다. IPC dispatcher와 실제 실행 엔진(run, custom IPC —
//! 옵션 폴링 포함)은 호스트가 본 크레이트의 API를 호출해 조율한다.
#![allow(clippy::result_large_err)]

pub mod barrier;
pub mod lease;
pub mod platform;
pub mod rate_limit;
pub mod reducer;
pub mod runner;
pub mod semaphore;
pub mod task;

use thiserror::Error;

pub use barrier::{Barrier, BarrierState, BarrierStore};
pub use lease::{Lease, LeaseMode, LeaseStore};
pub use rate_limit::{ConsumeOutcome, RateLimit, RateLimitStore};
pub use reducer::{ReducerInput, reduce_in_process, reduce_with_custom};
pub use runner::{DispatchHandle, DispatchOutcome, PollOutcome, RunnerLoop, TaskExecutor};
pub use semaphore::{AcquireOutcome, ReleaseOutcome, Semaphore, SemaphoreStore};
pub use task::{
    InlineFallbackSpec, OnFailure, PollSpec, PollSpecRef, ReducerStrategy, Task, TaskCommand,
    TaskGraph, TaskId, TaskResult, TaskState, TaskStore,
};

/// 본 크레이트의 공용 에러.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("task not found: {0}")]
    TaskNotFound(TaskId),
    #[error("dependency cycle through tasks: {0:?}")]
    DependencyCycle(Vec<TaskId>),
    #[error("unknown dependency task: {0}")]
    UnknownDependency(TaskId),
    #[error("invalid state transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },
    #[error("task already in terminal state: {0}")]
    AlreadyTerminal(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("lease conflict: '{resource}' held by '{holder}'")]
    LeaseConflict { resource: String, holder: String },
    /// task 삭제 시 다른 task 가 여전히 참조 중(`depends_on`/`Fallback.task`/
    /// `Reduce.inputs`) — `--cascade`/`--force` 없이는 거부. `referenced_by` 를
    /// 응답 `error.data` 에 실어 호출자가 다음 행동을 정할 수 있게 한다.
    #[error("task {task} is referenced by: {referenced_by:?}")]
    TaskReferenced {
        task: TaskId,
        referenced_by: Vec<TaskId>,
    },
    /// task 삭제 금지 상태는 `Running` 하나뿐 — `cancel` 로 먼저 정리해야 한다.
    /// `--force` 도 이 제약은 뚫지 못한다.
    #[error("cannot delete task {0} while it is Running — cancel it first")]
    TaskRunning(TaskId),
    #[error("memory: {0}")]
    Memory(#[from] tasty_memory::MemoryError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;
