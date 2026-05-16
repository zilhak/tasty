//! Tasty Agent — 다중 에이전트 협업 primitive (Phase 5).
//!
//! 1차 시민으로 다음을 제공:
//! - **Task DAG**: 의존성을 가진 task 그래프. state 머신과 사이클 검출.
//! - (후속) Barrier / Semaphore / Lease / Reducer / Rate Limit.
//!
//! 영속은 `tasty-memory` 위에 얹는다 (scope = workspace, key prefix =
//! `tasty.agent.task.<id>`). 본 크레이트는 GUI/IPC와 독립적이며 순수 상태 머신
//! + 영속 헬퍼만 담당한다. IPC dispatcher와 실제 실행 엔진(claude.spawn, run,
//! custom IPC)은 호스트가 본 크레이트의 API를 호출해 조율한다.

#![allow(clippy::result_large_err)]

pub mod barrier;
pub mod lease;
pub mod semaphore;
pub mod task;

pub use barrier::{Barrier, BarrierState, BarrierStore};
pub use lease::{Lease, LeaseMode, LeaseStore};
pub use semaphore::{AcquireOutcome, ReleaseOutcome, Semaphore, SemaphoreStore};
pub use task::{
    OnFailure, ReducerStrategy, Task, TaskCommand, TaskGraph, TaskId, TaskResult, TaskState,
    TaskStore,
};

use thiserror::Error;

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
    #[error("memory: {0}")]
    Memory(#[from] tasty_memory::MemoryError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;
