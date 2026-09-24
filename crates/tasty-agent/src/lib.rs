//! 작업 DAG와 barrier·semaphore·lease·reducer·rate limit을 제공한다.
//!
//! 상태와 영속 처리는 tasty-memory를 사용하며 GUI·IPC 실행은 호스트가 담당한다.
//! custom reducer에는 셸 실행 함수를 주입할 수 있고, run_custom_shell을 기본값으로 제공한다.
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
pub use lease::{AcquireAnyOutcome, ElasticSpec, Lease, LeaseMode, LeaseStore};
pub use rate_limit::{ConsumeOutcome, RateLimit, RateLimitStore};
pub use reducer::{
    ReducerInput, extract_paths, reduce_in_process, reduce_with_custom, run_custom_shell,
};
pub use runner::{DispatchHandle, DispatchOutcome, PollOutcome, RunnerLoop, TaskExecutor};
pub use semaphore::{AcquireOutcome, ReleaseOutcome, Semaphore, SemaphoreHolder, SemaphoreStore};
pub use task::{
    DagStateCounts, DagSummary, InlineFallbackSpec, OnFailure, PollSpec, PollSpecRef,
    ReducerStrategy, Task, TaskCommand, TaskGraph, TaskId, TaskResult, TaskState, TaskStore,
    group_tasks_into_dags,
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
    /// pool 모드(`LeaseStore::acquire_any`)에서 fixed candidates 가 전부
    /// 점유 중이거나(elastic 이면) `max_candidates` 상한까지 합성해도 빈
    /// 자리가 없을 때 `LeaseMode::Fail` 에서 반환.
    #[error("lease pool exhausted: none of {candidates:?} available for '{holder}'")]
    LeasePoolExhausted {
        candidates: Vec<String>,
        holder: String,
    },
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

/// 호출자 값으로 memory 키를 만들고 입력 오류를 InvalidArgument로 반환한다.
/// 접두사가 붙은 내부 키 대신 호출자 값의 문자 위치를 오류에 표시한다.
/// 허용 문자 검사는 tasty_memory를 사용한다. 빈 값도 접두사만으로 키가 유효하면 허용한다.
pub(crate) fn component_key(prefix: &str, label: &str, value: &str) -> Result<String> {
    let key = format!("{prefix}{value}");
    if let Err(full) = tasty_memory::validate_key(&key) {
        let budget = tasty_memory::MAX_KEY_LEN.saturating_sub(prefix.len());
        // validate_key의 바이트 위치 대신 입력 문자의 위치를 보고한다.
        let mut buf = [0u8; 4];
        let bad = value
            .chars()
            .enumerate()
            .find(|(_, c)| tasty_memory::validate_key(c.encode_utf8(&mut buf)).is_err());
        let reason = match bad {
            Some((i, c)) => format!("invalid char at {i}: {c:?}"),
            None if !value.is_empty() => format!("too long: {} bytes > {budget}", value.len()),
            None => full,
        };
        return Err(AgentError::InvalidArgument(format!(
            "{label} {value:?}: {reason} (allowed: {}; at most {budget} bytes)",
            tasty_memory::KEY_ALLOWED_CHARS
        )));
    }
    Ok(key)
}

#[cfg(test)]
mod component_key_tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use tasty_memory::MemoryStore;

    fn mem() -> MemoryStore {
        MemoryStore::open_in_memory().unwrap()
    }

    fn assert_caller_error(err: AgentError, label: &str) {
        let AgentError::InvalidArgument(msg) = &err else {
            panic!("expected InvalidArgument, got {err:?}");
        };
        assert!(msg.starts_with(label), "{msg}");
        assert!(msg.contains("\"v6S\""), "{msg}");
        assert!(msg.contains("invalid char at 2: 'S'"), "{msg}");
    }

    #[test]
    fn semaphore_name_is_judged_in_caller_coordinates() {
        let mut m = mem();
        let mut store = SemaphoreStore::new(&mut m, "_host");
        assert_caller_error(store.create(1, "v6S", 1, 0).unwrap_err(), "semaphore name");
        assert_caller_error(
            store.acquire(1, "v6S", "h", None, 0).unwrap_err(),
            "semaphore name",
        );
        assert_caller_error(store.delete(1, "v6S").unwrap_err(), "semaphore name");
    }

    #[test]
    fn barrier_rate_limit_and_task_ids_are_judged_the_same_way() {
        let mut m = mem();
        assert_caller_error(
            BarrierStore::new(&mut m, "_host")
                .create(1, "v6S", 1, None, 0)
                .unwrap_err(),
            "barrier name",
        );
        assert_caller_error(
            RateLimitStore::new(&mut m, "_host")
                .remove("v6S")
                .unwrap_err(),
            "rate_limit id",
        );
        let seq = AtomicU64::new(0);
        assert_caller_error(
            TaskStore::new(&mut m, "_host", &seq)
                .get(1, &"v6S".to_string())
                .unwrap_err(),
            "task id",
        );
    }

    #[test]
    fn a_multibyte_char_is_named_as_typed_and_counted_in_chars() {
        let mut m = mem();
        let mut store = SemaphoreStore::new(&mut m, "_host");
        for (name, want) in [
            ("aé", "invalid char at 1: 'é'"),
            ("ab한글", "invalid char at 2: '한'"),
        ] {
            let err = store.create(1, name, 1, 0).unwrap_err();
            let AgentError::InvalidArgument(msg) = &err else {
                panic!("expected InvalidArgument, got {err:?}");
            };
            assert!(msg.contains(want), "{name}: {msg}");
        }
    }

    #[test]
    fn too_long_name_reports_the_budget_left_after_the_prefix() {
        let mut m = mem();
        let mut store = SemaphoreStore::new(&mut m, "_host");
        let budget = tasty_memory::MAX_KEY_LEN - semaphore::SEMAPHORE_KEY_PREFIX.len();
        store.create(1, "a".repeat(budget), 1, 0).unwrap();
        let err = store.create(1, "a".repeat(budget + 1), 1, 0).unwrap_err();
        let AgentError::InvalidArgument(msg) = &err else {
            panic!("expected InvalidArgument, got {err:?}");
        };
        assert!(
            msg.contains(&format!("too long: {} bytes > {budget}", budget + 1)),
            "{msg}"
        );
    }
}
