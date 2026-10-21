//! runner·worker 스레드가 memory writer 순서 안에서 저장소를 쓰는 방법. 본문은 writer 스레드에서
//! 실행되므로 필요한 값을 소유해 가져가고, 호출 스레드는 결과가 나올 때까지 기다린다
//! ([`tasty_memory::writer::MemoryAccess::run_blocking`]).
//! 메인 루프는 이 경로를 쓰지 않고 writer 요청과 continuation으로 처리한다.

use tasty_agent::AgentError;
use tasty_memory::writer::{Blocked, MemoryAccess};
use tasty_memory::{MemoryError, MemoryStorage};

/// writer가 본문의 결과를 확정하지 못했을 때 호출자에게 돌려줄 값. 결과형이 저장소 오류나 없는 값을
/// 이미 표현하는 경우만 구현한다. bool처럼 실패의 뜻을 정할 수 없는 결과형은 호출자가
/// [`Blocked`]를 보고 직접 정한다([`crate::runner_host::RunnerContext::store`]).
pub trait FromBlocked {
    fn from_blocked(blocked: Blocked) -> Self;
}

impl<T> FromBlocked for Result<T, AgentError> {
    fn from_blocked(blocked: Blocked) -> Self {
        Err(AgentError::Memory(blocked.to_memory_error()))
    }
}

impl<T> FromBlocked for Result<T, MemoryError> {
    fn from_blocked(blocked: Blocked) -> Self {
        Err(blocked.to_memory_error())
    }
}

impl<T> FromBlocked for Result<T, String> {
    fn from_blocked(blocked: Blocked) -> Self {
        Err(blocked.to_string())
    }
}

/// 읽기 실패를 없는 값으로 다루던 본문(`.ok().flatten()`)의 결과형. 경고를 남기고 없는 값이다.
impl<T> FromBlocked for Option<T> {
    fn from_blocked(blocked: Blocked) -> Self {
        tracing::warn!(error = %blocked, "memory read not answered; treated as absent");
        None
    }
}

/// 읽기 실패를 빈 목록으로 다루던 본문(`.unwrap_or_default()`)의 결과형.
impl<T> FromBlocked for Vec<T> {
    fn from_blocked(blocked: Blocked) -> Self {
        tracing::warn!(error = %blocked, "memory read not answered; treated as empty");
        Vec::new()
    }
}

/// 실패를 본문 안에서 로그로 처리하던 본문의 결과형.
impl FromBlocked for () {
    fn from_blocked(blocked: Blocked) -> Self {
        tracing::warn!(error = %blocked, "memory request not answered");
    }
}

/// 본문을 writer에서 실행하고 결과를 기다린다. 확정하지 못하면 결과형의 저장소 오류다.
pub(crate) fn blocking<R: FromBlocked + Send + 'static>(
    access: &MemoryAccess,
    body: impl Fn(&mut dyn MemoryStorage) -> R + Send + Sync + 'static,
) -> R {
    access.run_blocking(0, body).unwrap_or_else(R::from_blocked)
}
